Maintained by [Ankara Yazılım](https://github.com/Ankara-Software) | [Project page](https://ankarayazilim.org/acik-kaynak/logrustate)

**Status: beta.** It builds, the 5 unit tests pass and rotation, compression and dry-run were run by hand on macOS. It supports a subset of the logrotate directives, and a few that are parsed are not applied yet (see [Limitations](#limitations)).

<div align="center">

<h1>logrustate</h1>

<p><strong>A log rotation tool that reads TOML or logrotate-style configuration — written in Rust.</strong></p>

[![License: MIT](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![GitHub Release](https://img.shields.io/github/v/release/Ankara-Software/logrustate?style=flat-square&color=8be04e)](https://github.com/Ankara-Software/logrustate/releases)

```bash
logrustate /etc/logrustate.toml --verbose
```

</div>

## Türkçe

logrustate, `logrotate` benzeri bir log döndürme aracıdır. Ayarları TOML dosyasından ya da `logrotate.conf` benzeri klasik söz diziminden okur. Dosyaları günlük, haftalık, aylık veya boyuta göre döndürür, gzip ile sıkıştırır ve `--debug` ile hiçbir dosyaya dokunmadan ne yapacağını gösterir. Proje beta aşamasındadır: logrotate direktiflerinin yalnızca bir bölümünü destekler ve bazı direktifler (`mail`, `maxage`) okunur ama henüz uygulanmaz.

---

## Why logrustate?

`logrotate` is the standard log rotation tool on Linux. logrustate offers the same kind of job with a TOML configuration, verbose output, a dry-run mode (`--debug`) and support for the common legacy `logrotate.conf` syntax, in a single Rust binary.

---

## Installation

### Install script (Linux / macOS)
```bash
curl -fsSL https://raw.githubusercontent.com/Ankara-Software/logrustate/master/install.sh | sh
```

### Cargo (Rust 1.85+)
```bash
cargo install --git https://github.com/Ankara-Software/logrustate
```
The crate is also published on crates.io as `logrustate-cli`. The published 0.1.2 installs a binary named `logrustate-cli`; building from this repository installs `logrustate`. The crates.io name `logrustate` belongs to a different project.

---

## Quick Start

### Create a TOML config
Entries are `[[entries]]` tables with a `paths` list. Shared defaults go in an optional `[global]` table. A file is read as TOML only when its name ends in `.toml`; any other file is read as legacy `logrotate.conf` syntax. Unknown keys are ignored without a warning, so check that the verbose output reports the number of blocks you expect.

```toml
# /etc/logrustate.toml

[global]
rotate = 7

[[entries]]
paths = ["/var/log/nginx/*.log"]
frequency = "daily"   # hourly, daily, weekly, monthly, yearly
compress = true       # gzip after rotation
missingok = true      # no error if the file is missing
notifempty = true     # skip rotation if the log is empty
```

The same block in legacy syntax:
```
/var/log/nginx/*.log {
    rotate 7
    daily
    compress
    missingok
    notifempty
}
```

### Preview without changes (dry-run)
```bash
logrustate /etc/logrustate.toml --debug
```

### Options
```
Usage: logrustate [OPTIONS] <CONFIGS>...

  -d, --debug           Debug mode: implies --verbose, does not modify any files (dry-run)
  -v, --verbose         Verbose mode: print details during processing
  -f, --force           Force rotation of all logs, even if not yet scheduled
  -s, --state <FILE>    Path to the state file [default: /var/lib/logrustate/status]
```
The state file records when each log was last rotated. Use `--state` to point it somewhere writable when you do not run as root.

## Limitations

- Supported directives: `rotate`, `hourly`/`daily`/`weekly`/`monthly`/`yearly` (TOML key `frequency`), `size`, `minsize`, `maxsize`, `compress`, `delaycompress`, `copytruncate`, `create`, `missingok`, `notifempty`, `dateext`, `dateformat`, `olddir`, `prerotate`, `postrotate`, `sharedscripts`, `include`.
- `mail` and `maxage` are parsed but not applied.
- Verified by hand on macOS only; not run against a real `/etc/logrotate.d` on Linux.

## Development

```bash
cargo build
cargo test
```

See [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## License

MIT. Copyright (c) 2026 Anilcan Kara. See [LICENSE](LICENSE).
