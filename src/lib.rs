//! # trapmail: A sendmail replacement for integration testing
//!
//! `trapmail` is a `sendmail` replacement for unit- and integration-testing that captures incoming
//! mail and stores it on the filesystem. Test cases can inspect the "sent" mails.
//!
//! ## Use case
//!
//! `trapmail` is intended for black-box testing systems that use the systemwide `sendmail` instance
//! to send emails. Example:
//!
//! 1. `trapmail` is installed and either replaces `sendmail` on the test system/container, or the
//!    application being tested is configured to use `trapmail` as its `sendmail` binary.
//! 2. An integration test (written in Rust) triggers various processes that cause the application
//!    to send mail, which is collected inside `TRAPMAIL_STORE`.
//! 3. Having access to `TRAPMAIL_STORE` as well, the `trapmail` library can be used inside the
//!    integration test to check if mail was queued as expected.
//!
//! ## CLI
//!
//! `trapmail`'s commandline aims to mimick the original `sendmail` arguments, commonly also
//! implemented by other [MTA](https://en.wikipedia.org/wiki/Message_transfer_agent)s like
//! Exim or Postfix.
//!
//! When `trapmail` receives a message, it stores it along with metadata a JSON file in the
//! directory named in the `TRAPMAIL_STORE` environment variable, falling back to `/tmp` if
//! not found. Files are named `trapmail_PPID_PID_TIMESTAMP.json`, where `PPID` is the parent
//! process' PID, `PID` trapmails `PID` at the time of the call and `TIMESTAMP` a microsecond
//! accurate timestamp.
//!
//! ### Command-line options
//!
//! Currently, `trapmail` does not "support" all the same command-line options that sendmail
//! supports (all options are ignored, but logged). If you run into issues due to an
//! unsupported option, feel free to open a PR to get it added.
//!
//! ### Example
//!
//! ```text
//! $ trapmail --debug -i -t foo@bar
//! To: Santa Clause <santa@example.com>
//! From: Marc <marc@example.com>
//! Subject: Please remove me from the naughty list.
//!
//! Example body.
//! ^D
//! Mail written to "/tmp/trapmail_1575911147313470_5913_6299.json"
//! ```
//!
//! The resulting mail is (somewhat readable) JSON, but can also be dumped using the cli tool:
//!
//! ```text
//! $ trapmail --dump /tmp/trapmail_1575911147313470_5913_6299.json
//! Mail sent on 2019-12-09 17:05:47.000313 UTC from PID 6299 (PPID 5913).
//! CliOptions {
//!     debug: true,
//!     ignore_dots: true,
//!     inline_recipients: true,
//!     addresses: [
//!         "foo@bar",
//!     ],
//!     dump: None,
//! }
//! To: Santa Claus <santa@example.com>
//! From: Marc <marc@example.com>
//! Subject: Please remove me from the naughty list.
//!
//! Example body.
//! ```
//!
//! ## Concurrency
//!
//! While `trapmail` avoids collisions between stored messages from different processes due to its
//! naming scheme, it is important to remember that it has no way to access any data of the test
//! itself (the PPID is from the application-under-tests's PID, not the test binary).
//!
//! Providing different `TRAPMAIL_STORE` targets allows for namespacing the data, but it may not
//! always be possible to ensure this variable is set per test on a closed system.
//!
//! ## API
//!
//! The `trapmail` crate comes with a command-line application as well as a library. The
//! library can be used in tests and applications to access all data that `trapmail` writes.
//!
//! A minimal example to read the contents of the current trapmail folder:
//!
//! ```no_exec
//! use trapmail::MailStore;
//!
//! // Load mail from the default mail directory.
//! let store = MailStore::new();
//!
//! for load_result in store.iter_mails().expect("could not open mail store") {
//!     let mail = load_result.expect("could not load mail from file");
//!     println!("{}", mail);
//! }
//! ```
pub mod serde_pid;
mod util;

use crate::util::FlattenResultsIter;
use displaydoc::Display;
use lazy_static::lazy_static;
use nix::unistd::Pid;
use regex::Regex;
use serde_derive::{Deserialize, Serialize};
use std::convert::TryInto;
use std::{env, fmt, fs, io, path, thread, time};
use structopt::{clap, StructOpt};
use thiserror::Error;

/// Name of the environment variable indicating where to store mail.
pub const ENV_MAIL_STORE_PATH: &str = "TRAPMAIL_STORE";

/// Path to use in absence of `ENV_MAIL_STORE_PATH`.
const DEFAULT_MAIL_STORE_PATH: &str = "/tmp";

lazy_static! {
    /// Regular expression that matches filenames generated by `Mail`.
    static ref FILENAME_RE: Regex = Regex::new(r"trapmail_\d+_\d+_\d+.json").unwrap();
}

/// Command-line options for the `trapmail` program.
#[derive(Clone, Debug, Deserialize, Serialize, StructOpt)]
pub struct CliOptions {
    /// Non-standard debug output (outputs trapmail-specific debug info to `stderr`)
    #[structopt(long = "debug")]
    pub debug: bool,
    /// Ignore dots alone on lines by themselves in incoming message
    #[structopt(short = "i")]
    pub ignore_dots: bool,
    /// Read message for recipient list
    #[structopt(short = "t")]
    pub inline_recipients: bool,
    /// Additional command line options
    pub options: Vec<String>,
    /// Ignore everything else and dump the contents of an email file instead.
    #[structopt(long = "dump")]
    pub dump: Option<path::PathBuf>,
    /// Set option option to the specified value. (e.g. `-O foo=bar`)
    #[structopt(short = "O")]
    pub option: Vec<String>,
    /// Sets the name of the ''from'' person (i.e., the envelope sender of the mail).
    #[structopt(short = "f")]
    pub sender: String,
    /// The mail store path. Overrides the eponymous environment variable.
    #[structopt(long = "store-path")]
    pub store_path: Option<String>,
}

impl CliOptions {
    /// Get CLI options from actual command line.
    pub fn from_args() -> Self {
        let app = Self::clap().setting(clap::AppSettings::AllowLeadingHyphen);
        Self::from_clap(&app.get_matches())
    }
}

/// A trapmail error.
#[derive(Debug, Display, Error)]
pub enum Error {
    /// "Could not store mail: {0}
    Store(io::Error),
    /// "Could not serialize mail: {0}
    MailSerialization(serde_json::Error),
    /// "Could enumerate storage directory: {0}
    DirEnumeration(util::DirReadError),
    /// "Could not load mail: {0}
    Load(io::Error),
    /// "Could not deserialize mail: {0}
    MailDeserialization(serde_json::Error),
}

type Result<T> = ::std::result::Result<T, Error>;

/// An email body.
///
/// Mail bodies *should* be 7-bit ASCII (which is a subset of UTF-8), but there is no guarantee that
/// clients/callers send valid data.
///
/// Upon creation, the body is parsed and stored as one of either variant, this allows the JSON
/// serialization to be ideally human-readable, if valid UTF8 is used (i.e. one can have a quick
/// look at mail contents in a text editor).
#[derive(Debug, Deserialize, Serialize)]
pub enum MailBody {
    /// A valid UTF8-formatted mail.
    Utf8(String),
    /// A mail containing non-UTF8 characters.
    Invalid(#[serde(with = "serde_bytes")] Vec<u8>),
}

impl MailBody {
    /// Create new `MailBody` from raw input bytes.
    #[inline]
    fn from_raw(raw_body: Vec<u8>) -> Self {
        match String::from_utf8(raw_body) {
            Ok(s) => MailBody::Utf8(s),
            Err(e) => MailBody::Invalid(e.into_bytes()),
        }
    }
}

impl fmt::Display for MailBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MailBody::Utf8(s) => write!(f, "{}", s),
            MailBody::Invalid(raw) => write!(f, "[invalid UTF-8]{}", String::from_utf8_lossy(&raw)),
        }
    }
}

/// A "sent" mail.
#[derive(Debug, Deserialize, Serialize)]
pub struct Mail {
    /// The command line arguments passed to `trapmail` at the time of call.
    pub cli_options: CliOptions,
    /// The ID of the `trapmail` process that stored this email.
    #[serde(with = "serde_pid")]
    pub pid: Pid,
    /// The ID of the parent process that called `trapmail`.
    #[serde(with = "serde_pid")]
    pub ppid: Pid,
    /// The `trapmail` call's raw body.
    pub body: MailBody,
    /// A microsecond-resolution UNIX timestamp of when the mail arrived.
    pub timestamp_us: u128,
}

impl Mail {
    /// Create a new `Mail` using the current time and process information.
    ///
    /// This function will sleep for a microsecond to avoid any conflicts in
    /// naming (see `file_name`).
    ///
    /// # Panics
    ///
    /// Will panic if the system returns a time before the UNIX epoch.
    pub fn new(cli_options: CliOptions, raw_body: Vec<u8>) -> Self {
        // We always sleep a microsecond, which is probably overkill, but
        // guarantees no collisions, ever (a millions mails a second ought
        // to be enough for even future test cases).
        thread::sleep(time::Duration::from_nanos(1000));

        let timestamp_us = (time::SystemTime::now().duration_since(time::UNIX_EPOCH))
            .expect("Got current before 1970; is your clock broken?")
            .as_micros();

        Mail {
            cli_options,
            body: MailBody::from_raw(raw_body),
            pid: nix::unistd::Pid::this(),
            ppid: nix::unistd::Pid::parent(),
            timestamp_us,
        }
    }

    /// Create a (pathless) file_name depending on the `Mail` contents.
    pub fn file_name(&self) -> path::PathBuf {
        format!(
            "trapmail_{}_{}_{}.json",
            self.timestamp_us, self.ppid, self.pid,
        )
        .into()
    }

    /// Load a `Mail` from a file.
    pub fn load<P: AsRef<path::Path>>(source: P) -> Result<Self> {
        serde_json::from_reader(fs::File::open(source).map_err(Error::Load)?)
            .map_err(Error::MailDeserialization)
    }
}

/// Convert microsecond timestamp to `chrono::NaiveDateTime`.
///
/// Returns an error if input data is out-of-range for the underlying type.
fn us_to_datetime(
    timestamp_us: u128,
) -> ::core::result::Result<chrono::NaiveDateTime, core::num::TryFromIntError> {
    Ok(chrono::NaiveDateTime::from_timestamp(
        (timestamp_us / 1_000_000).try_into()?,
        (timestamp_us % 1_000_000).try_into()?,
    ))
}

impl fmt::Display for Mail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let formatted_timestamp = if let Ok(dt) = us_to_datetime(self.timestamp_us) {
            dt.format("%Y-%m-%d %H:%M:%S%.6f").to_string()
        } else {
            format!("[cannot convert {} to timestamp]", self.timestamp_us)
        };

        write!(
            f,
            "Mail sent on {} UTC from PID {} (PPID {}).\n\
             {:#?}\n\
             {}",
            formatted_timestamp, self.pid, self.ppid, self.cli_options, self.body
        )
    }
}

/// Mail storage.
#[derive(Debug)]
pub struct MailStore {
    /// Root path where all mail in this store gets stored.
    root: path::PathBuf,
}

impl MailStore {
    /// Construct a new `MailStore`.
    ///
    /// The path will be set from the environment or use a default, if not set.
    #[inline]
    pub fn new() -> Self {
        MailStore::with_root(
            env::var(ENV_MAIL_STORE_PATH).unwrap_or_else(|_| DEFAULT_MAIL_STORE_PATH.to_owned()),
        )
    }

    /// Construct a new `MailStore` with given path.
    #[inline]
    pub fn with_root<P: Into<path::PathBuf>>(root: P) -> Self {
        MailStore { root: root.into() }
    }

    /// Add a mail to the `MailStore`.
    ///
    /// Returns the path where the mail has been stored.
    pub fn add(&self, mail: &Mail) -> Result<path::PathBuf> {
        let output_fn = self.root.join(mail.file_name());

        serde_json::to_writer_pretty(fs::File::create(&output_fn).map_err(Error::Store)?, mail)
            .map_err(Error::MailSerialization)?;
        Ok(output_fn)
    }

    /// Iterate over all mails in storage.
    ///
    /// Mails are ordered by timestamp.
    pub fn iter_mails(&self) -> impl Iterator<Item = Result<Mail>> {
        util::read_dir_matching(&self.root, &FILENAME_RE)
            .map_err(Error::DirEnumeration)
            .map(|paths| paths.into_iter().map(Mail::load))
            .flatten_results()
    }
}

impl Default for MailStore {
    fn default() -> Self {
        Self::new()
    }
}
