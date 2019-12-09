use failure::bail;
use std::io;
use std::io::Read;
use structopt::StructOpt;

fn main() -> Result<(), failure::Error> {
    let opt = trapmail::CliOptions::from_args();

    // If given the `--dump` flag, we ignore everything and just dump the mail.
    if let Some(target) = opt.dump {
        let mail = trapmail::Mail::load(target)?;
        // TODO: Try to format U8 a little better here, possibly using FmtU8
        //       `byte_string`.
        println!("{:#?}", mail);
        return Ok(());
    }

    if !opt.ignore_dots {
        bail!("ignore dots (`-i`) was not set, but the reverse is not supported");
    }

    if !opt.inline_recipients {
        bail!("inline recipients (`-t`) was not set, but the reverse is not supported");
    }

    let store = trapmail::MailStore::new();

    // Read stdin as the mail and store it. All the parsing is handled by the
    // process running the test cases.
    let mut buffer = Vec::new();
    io::stdin().read_to_end(&mut buffer)?;

    let mail = trapmail::Mail::new(opt.clone(), buffer);
    let storage_path = store.add(&mail)?;

    if opt.debug {
        eprintln!("Mail written to {:?}", storage_path);
    }

    Ok(())
}
