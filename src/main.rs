use anyhow::Result;
use async_imap::{extensions::idle::IdleResponse, Session};
use async_native_tls::TlsStream;
use clap::Parser;
use futures::{future::join_all, StreamExt};
use ntfy::{Dispatcher, Payload, Priority, Url};
use serde::Deserialize;
use std::{fs, io::Read, path::PathBuf, time::Duration};
use tokio::{net::TcpStream, task, time::sleep};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    config: PathBuf,
}

#[derive(Deserialize)]
struct Account {
    name: String,
    server: String,
    port: u16,
    username: String,
    password: String,
    ntfy_url: String,
    ntfy_topic: String,
    ntfy_clickable_url: Option<String>,
}

#[derive(Deserialize)]
struct Config {
    accounts: Vec<Account>,
}

struct UnseenMail {
    account: Account,
}

impl UnseenMail {
    async fn check_once(
        &self,
        session: &mut Session<TlsStream<TcpStream>>,
        last_notified: &mut u32,
    ) -> Result<()> {
        let mut uids = session.uid_search("NEW 1:*").await?;
        if uids.iter().all(|&uid| uid <= *last_notified) {
            // there are no messages we haven't already notified about
            uids.clear();
        }
        *last_notified = std::cmp::max(*last_notified, uids.iter().cloned().max().unwrap_or(0));
        let uids: Vec<_> = uids.into_iter().map(|v: u32| format!("{}", v)).collect();
        let msg_stream = session.uid_fetch(uids.join(","), "RFC822.HEADER").await?;
        let msgs = msg_stream.collect::<Vec<_>>().await;
        println!("-- number of fetched msgs: {:?}", msgs.len());
        for msg in msgs {
            let msg = msg?;
            let msg = msg.header();
            if msg.is_none() {
                continue;
            }
            match mailparse::parse_headers(msg.unwrap()) {
                Ok((headers, _)) => {
                    use mailparse::MailHeaderMap;
                    let subject = headers
                        .get_first_value("Subject")
                        .unwrap_or_else(|| String::from("<no subject>"));
                    println!("new mail: {}", subject);
                    self.send_new_mail_notification(&subject).await.ok();
                }
                Err(e) => {
                    println!("failed to parse headers of message: {:?}", e);
                }
            }
        }
        Ok(())
    }

    async fn idle_wait(
        &self,
        session: Session<TlsStream<TcpStream>>,
    ) -> Result<Session<TlsStream<TcpStream>>> {
        // init idle session
        println!("-- initializing idle");
        let mut idle = session.idle();
        idle.init().await?;

        println!("-- idle async wait");
        let (idle_wait, interrupt) = idle.wait();

        task::spawn(async move {
            println!("-- thread: waiting for 300 secs");
            sleep(Duration::from_secs(300)).await;
            println!("-- thread: waited 300 secs, now interrupting idle");
            drop(interrupt);
        });

        let idle_result = idle_wait.await?;
        match idle_result {
            IdleResponse::ManualInterrupt => {
                println!("-- IDLE manually interrupted");
            }
            IdleResponse::Timeout => {
                println!("-- IDLE timed out");
            }
            IdleResponse::NewData(data) => {
                let s = String::from_utf8(data.borrow_raw().to_vec()).unwrap();
                println!("-- IDLE data:\n{}", s);
            }
        }

        // return the session after we are done with it
        println!("-- sending DONE");
        let session = idle.done().await?;
        Ok(session)
    }

    async fn new_session(&self) -> Result<Session<TlsStream<TcpStream>>> {
        let account = &self.account;
        let tcp_stream = TcpStream::connect((account.server.as_str(), account.port)).await?;
        let tls = async_native_tls::TlsConnector::new();
        let tls_stream = tls.connect(account.server.as_str(), tcp_stream).await?;
        let client = async_imap::Client::new(tls_stream);
        println!("-- connected to {}:{}", account.server, account.port);

        let mut session = client
            .login(account.username.as_str(), account.password.as_str())
            .await
            .map_err(|e| e.0)?;
        println!("-- logged in a {}", account.username);

        let capabilities = session.capabilities().await?;
        if !capabilities.has_str("IDLE") {
            panic!("server does not support IDLE (in [{}])", self.account.name);
        }

        session.select("INBOX").await?;
        println!("-- INBOX selected");
        Ok(session)
    }

    async fn loop_check(
        &self,
        mut session: Session<TlsStream<TcpStream>>,
        last_notified: &mut u32,
    ) -> Result<()> {
        loop {
            let check_result = self.check_once(&mut session, last_notified).await;
            if check_result.is_err() {
                // be nice to the server and log out
                eprintln!("-- check failed and logging out");
                session.logout().await?;
            }
            check_result?;
            session = self.idle_wait(session).await?;
        }
    }

    async fn send_new_mail_notification(&self, subject: &str) -> Result<()> {
        let dispatcher = Dispatcher::builder(&self.account.ntfy_url).build()?;
        let mut payload = Payload::new(&self.account.ntfy_topic)
            .title(format!("@{} has new mail", self.account.name))
            .message(subject)
            .priority(Priority::Default);
        if let Some(ntfy_clickable_url) = &self.account.ntfy_clickable_url {
            payload = payload.click(Url::parse(ntfy_clickable_url).unwrap());
        }
        dispatcher.send(&payload).await?;
        Ok(())
    }

    async fn report_error(&self, error_msg: &str) -> Result<()> {
        let dispatcher = Dispatcher::builder(&self.account.ntfy_url).build()?;
        let payload = Payload::new(&self.account.ntfy_topic)
            .title(format!("@{} connection failed", self.account.name))
            .message(error_msg)
            .tags(vec!["warning".into()])
            .priority(Priority::Default);
        dispatcher.send(&payload).await?;
        Ok(())
    }

    async fn run(self) {
        let mut wait = 1u64;
        let mut last_notified = 0;
        loop {
            let session = self.new_session().await;
            match session {
                Ok(session) => {
                    self.loop_check(session, &mut last_notified).await.ok();
                }
                Err(e) => {
                    eprintln!(
                        "connection failed: {}; trying to reconnect after {wait}s ...",
                        e
                    );
                    if wait >= 256 {
                        self.report_error(&format!(
                            "connection failed: {}; trying to reconnect after {wait}s ...",
                            e
                        ))
                        .await
                        .ok();
                    }
                    sleep(Duration::from_secs(wait)).await;
                    wait *= 2;
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config_path = args.config;
    let mut buf = String::new();
    fs::File::open(config_path)
        .unwrap()
        .read_to_string(&mut buf)
        .unwrap();
    let config: Config = toml::from_str(&buf).unwrap();
    let accounts = config.accounts;
    let tasks = accounts
        .into_iter()
        .map(|account| UnseenMail { account }.run());
    join_all(tasks).await;
    Ok(())
}
