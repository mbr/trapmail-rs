# trapmail

`trapmail` is a `sendmail` replacement for unit- and integration-testing that captures incoming mail and stores it on the filesystem. Test cases can inspect the "sent" mails Its commandline aims to mimick the original `sendmail` arguments, commonly also implemented by other [MTA](https://en.wikipedia.org/wiki/Message_transfer_agent)s like Exim or Postfix.

When `trapmail` receives a message, it stores it along with metadata a JSON file in the directory named in the `TRAPMAIL_STORE` environment variable, falling back to `/tmp` if not found. Files are named `trapmail_PPID_PID_TIMESTAMP.json`, where `PPID` is the parent process' PID, `PID` trapmails `PID` at the time of the call and `TIMESTAMP` a microsecond accurate timestamp.

## Concurrency

While `trapmail` avoids collisions between stored messages, it cannot guarantee that other test processes/threads that are running simultaneously do not add to the store as well. Care must be taken to avoid race conditions, especially when clearing the mail store.

Providing different `TRAPMAIL_STORE` targets allows for namespacing the data. Otherwise, depending on the usecase, `PID` and `PPID` may aid in filtering.

## API

The `trapmail` crate comes with a command-line application as well as a library. The library can be used in tests and applications to access all data that `trapmail` writes.