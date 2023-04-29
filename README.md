# unseenmail | Notify via ntfy when unseen new emails arrive

I use this service to reduce k-9 mail power consumption. If you have new features, please feel free to PR.

## Requirements

- Mail service requires IMAP and IDLE support.
- Self-hosted [ntfy](https://github.com/binwiederhier/ntfy).

> I use QQ and Fastmail.

## Configuration

```toml
[[accounts]]
name = "example"
server = "imap.example.com"
port = 993
username = "example@example.com"
password = "password"
ntfy_url = "https://ntfy.example.com"
ntfy_topic = "new_mail"
ntfy_clickable_url = "k9mail://messages" # optional

[[accounts]]
name = "example2"
server = "imap.example2.com"
port = 993
username = "example2@example2.com"
password = "password"
ntfy_url = "https://ntfy.example2.com"
ntfy_topic = "new_mail_2"
ntfy_clickable_url = "k9mail://messages" # optional
```

## Installation

See [docker-compose.yml](docker-compose.yml).

Put the configuration file into `./app/unseenmail.toml`.

## Credits

- [buzz](https://github.com/jonhoo/buzz): A simple system tray application for notifying about unseen e-mail.
- [async-imap](https://github.com/async-email/async-imap):  Async IMAP implementation in Rust.
