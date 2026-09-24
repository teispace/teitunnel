//! Where the control connection lives and how both ends reach it.
//!
//! Everything sits in `<data dir>/control/`, a folder only the user can open (0700 on
//! macOS and Linux; the user's profile on Windows):
//!
//! - `token`: 32 random bytes as hex (0600), made once per install. A client presents
//!   it in `hello`: defence in depth on top of the file permissions.
//! - macOS/Linux: `sock`, a Unix domain socket (0600). The server also checks that each
//!   peer runs as the same user.
//! - Windows: `pipe` holds the name of a named pipe with a random suffix, created with
//!   a DACL that grants only the current user (and refusing remote clients). The random
//!   name means nobody can create the pipe before the app does.
//!
//! Nothing listens on TCP.

use std::{
    io,
    path::{Path, PathBuf},
    pin::Pin,
    time::Duration,
};

use subtle::ConstantTimeEq;
use tokio::io::{AsyncRead, AsyncWrite};

/// How long a client waits for the app to accept.
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);

/// A byte stream to the other end.
pub trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Stream for T {}

/// A connection, whatever the platform's transport.
pub type Connection = Pin<Box<dyn Stream>>;

/// The install's secret shared with clients (never logged: `Debug` is redacted).
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(…)")
    }
}

impl Token {
    fn generate() -> io::Result<Self> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Self(bytes.iter().map(|b| format!("{b:02x}")).collect()))
    }

    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        (text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| Self(text.to_ascii_lowercase()))
    }

    /// The token as sent in `hello`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `presented` is this token, in constant time.
    pub fn matches(&self, presented: &str) -> bool {
        self.0.as_bytes().ct_eq(presented.trim().as_bytes()).into()
    }
}

/// The control connection's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    dir: PathBuf,
}

impl Endpoint {
    /// The endpoint of the app whose data folder is `data_dir`.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("control"),
        }
    }

    /// The folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The token file.
    pub fn token_path(&self) -> PathBuf {
        self.dir.join("token")
    }

    /// The Unix domain socket.
    #[cfg(unix)]
    pub fn socket_path(&self) -> PathBuf {
        self.dir.join("sock")
    }

    /// The file naming the pipe.
    #[cfg(windows)]
    fn pipe_file(&self) -> PathBuf {
        self.dir.join("pipe")
    }

    /// Creates the folder (only the user can open it).
    fn create_dir(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    /// The install's token, created (0600) if there's none yet or it's unreadable.
    ///
    /// # Errors
    /// The folder or file can't be written.
    pub fn ensure_token(&self) -> io::Result<Token> {
        self.create_dir()?;
        if let Ok(token) = self.read_token() {
            restrict(&self.token_path())?;
            return Ok(token);
        }
        let token = Token::generate()?;
        let path = self.token_path();
        let partial = self.dir.join("token.partial");
        write_private(&partial, token.as_str())?;
        std::fs::rename(&partial, &path)?;
        Ok(token)
    }

    /// Reads the token (clients).
    ///
    /// # Errors
    /// There's no token file (the app never ran) or it's not a token.
    pub fn read_token(&self) -> io::Result<Token> {
        let text = std::fs::read_to_string(self.token_path())?;
        Token::parse(&text)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a control token"))
    }

    /// Starts listening (the app). Fails if another app instance already listens.
    ///
    /// # Errors
    /// The folder can't be prepared or the socket/pipe can't be created.
    pub async fn listen(&self) -> io::Result<Listener> {
        self.create_dir()?;
        platform::listen(self).await
    }

    /// Connects to the app (clients), within [`CONNECT_TIMEOUT`].
    ///
    /// # Errors
    /// The app isn't running (`NotFound`/`ConnectionRefused`) or didn't answer.
    pub async fn connect(&self) -> io::Result<Connection> {
        tokio::time::timeout(CONNECT_TIMEOUT, platform::connect(self))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "the app didn't answer"))?
    }
}

/// Writes `contents` to a new file only the user can read.
fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    use std::io::Write as _;
    let _ = std::fs::remove_file(path);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()
}

/// Makes an existing file readable by the user only.
fn restrict(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)?.permissions().mode() & 0o777;
        if mode != 0o600 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Accepts connections.
#[derive(Debug)]
pub struct Listener {
    inner: platform::Listener,
}

impl Listener {
    /// The next connection from a process of this user.
    ///
    /// # Errors
    /// Accepting failed (the caller keeps accepting after transient errors).
    pub async fn accept(&mut self) -> io::Result<Connection> {
        self.inner.accept().await
    }
}

#[cfg(unix)]
mod platform {
    use std::{io, os::unix::fs::MetadataExt, path::PathBuf};

    use tokio::net::{UnixListener, UnixStream};

    use super::{Connection, Endpoint};

    /// `sockaddr_un` holds 104 bytes on macOS (108 on Linux), terminator included.
    const MAX_SOCKET_PATH: usize = 103;

    #[derive(Debug)]
    pub(super) struct Listener {
        listener: UnixListener,
        path: PathBuf,
        uid: u32,
    }

    pub(super) async fn listen(endpoint: &Endpoint) -> io::Result<super::Listener> {
        use std::os::unix::fs::PermissionsExt;
        let path = endpoint.socket_path();
        check_length(&path)?;
        if path.exists() {
            // A socket that answers belongs to a running app; one that doesn't is left
            // over from a crash.
            if UnixStream::connect(&path).await.is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another Teitunnel is listening",
                ));
            }
            std::fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let uid = std::fs::metadata(endpoint.dir())?.uid();
        Ok(super::Listener {
            inner: Listener {
                listener,
                path,
                uid,
            },
        })
    }

    fn check_length(path: &std::path::Path) -> io::Result<()> {
        if path.as_os_str().len() > MAX_SOCKET_PATH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "the control socket's path is too long for a Unix socket: {}",
                    path.display()
                ),
            ));
        }
        Ok(())
    }

    impl Listener {
        pub(super) async fn accept(&mut self) -> io::Result<Connection> {
            loop {
                let (stream, _) = self.listener.accept().await?;
                // Folder permissions already keep other users out; this also refuses a
                // peer that somehow got in.
                match stream.peer_cred() {
                    Ok(cred) if cred.uid() == self.uid => return Ok(Box::pin(stream)),
                    Ok(cred) => {
                        tracing::warn!(uid = cred.uid(), "refused a control peer of another user");
                    }
                    Err(err) => tracing::warn!(%err, "couldn't identify a control peer"),
                }
            }
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    pub(super) async fn connect(endpoint: &Endpoint) -> io::Result<Connection> {
        let path = endpoint.socket_path();
        check_length(&path)?;
        Ok(Box::pin(UnixStream::connect(path).await?))
    }
}

#[cfg(windows)]
mod platform {
    use std::io;

    use interprocess::os::windows::{
        named_pipe::{PipeListenerOptions, pipe_mode, tokio::PipeListener},
        security_descriptor::SecurityDescriptor,
    };
    use tokio::net::windows::named_pipe::ClientOptions;

    use super::{Connection, Endpoint};

    const PREFIX: &str = r"\\.\pipe\teitunnel-control-";
    /// `ERROR_PIPE_BUSY`: every instance is in use; try again shortly.
    const PIPE_BUSY: i32 = 231;

    pub(super) struct Listener {
        listener: PipeListener<pipe_mode::Bytes, pipe_mode::Bytes>,
    }

    impl std::fmt::Debug for Listener {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Listener").finish_non_exhaustive()
        }
    }

    /// The SID of the user running this process (`S-1-5-21-…`).
    fn current_user_sid() -> io::Result<String> {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let pid = Pid::from_u32(std::process::id());
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().with_user(UpdateKind::Always),
        );
        let sid = system
            .process(pid)
            .and_then(|p| p.user_id())
            .map(|uid| (**uid).to_string())
            .ok_or_else(|| io::Error::other("couldn't read this user's SID"))?;
        if !sid.starts_with("S-1-") || !sid.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(io::Error::other("unexpected SID"));
        }
        Ok(sid)
    }

    pub(super) async fn listen(endpoint: &Endpoint) -> io::Result<super::Listener> {
        let mut suffix = [0u8; 16];
        getrandom::fill(&mut suffix).map_err(|e| io::Error::other(e.to_string()))?;
        let name: String = std::iter::once(PREFIX.to_owned())
            .chain(suffix.iter().map(|b| format!("{b:02x}")))
            .collect();
        // Protected DACL: full access for this user, nobody else (not even Everyone's
        // default read access to pipes).
        let sddl = format!("D:P(A;;GA;;;{})", current_user_sid()?);
        let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let sddl = widestring::U16CStr::from_slice(&wide)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
        let descriptor = SecurityDescriptor::deserialize(sddl)?;
        let listener = PipeListenerOptions::new()
            .path(name.as_str())
            .accept_remote(false)
            .security_descriptor(Some(descriptor))
            .create_tokio_duplex::<pipe_mode::Bytes>()?;
        let partial = endpoint.dir().join("pipe.partial");
        super::write_private(&partial, &name)?;
        std::fs::rename(&partial, endpoint.pipe_file())?;
        Ok(super::Listener {
            inner: Listener { listener },
        })
    }

    impl Listener {
        pub(super) async fn accept(&mut self) -> io::Result<Connection> {
            Ok(Box::pin(self.listener.accept().await?))
        }
    }

    pub(super) async fn connect(endpoint: &Endpoint) -> io::Result<Connection> {
        let name = std::fs::read_to_string(endpoint.pipe_file())?;
        let name = name.trim();
        if !name.starts_with(PREFIX) || name.len() != PREFIX.len() + 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a Teitunnel pipe",
            ));
        }
        loop {
            match ClientOptions::new().open(name) {
                Ok(client) => return Ok(Box::pin(client)),
                Err(err) if err.raw_os_error() == Some(PIPE_BUSY) => {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(err) => return Err(err),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_private_and_stable() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::new(dir.path());
        let token = endpoint.ensure_token().unwrap();
        assert_eq!(token.as_str().len(), 64);
        assert_eq!(
            endpoint.ensure_token().unwrap(),
            token,
            "made once per install"
        );
        assert_eq!(endpoint.read_token().unwrap(), token);
        assert!(token.matches(token.as_str()));
        assert!(!token.matches(&"0".repeat(64)));
        assert!(!token.matches(""));
        assert_eq!(format!("{token:?}"), "Token(…)");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&endpoint.token_path()), 0o600);
            assert_eq!(mode(endpoint.dir()), 0o700);
            // A token file someone loosened is tightened again.
            std::fs::set_permissions(
                endpoint.token_path(),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
            endpoint.ensure_token().unwrap();
            assert_eq!(mode(&endpoint.token_path()), 0o600);
        }
    }

    #[test]
    fn a_damaged_token_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::new(dir.path());
        std::fs::create_dir_all(endpoint.dir()).unwrap();
        std::fs::write(endpoint.token_path(), "nope").unwrap();
        assert!(endpoint.read_token().is_err());
        let token = endpoint.ensure_token().unwrap();
        assert_eq!(endpoint.read_token().unwrap(), token);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_socket_is_private_and_single() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::new(dir.path());
        let listener = endpoint.listen().await.unwrap();
        let mode = std::fs::metadata(endpoint.socket_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let second = endpoint.listen().await.unwrap_err();
        assert_eq!(second.kind(), io::ErrorKind::AddrInUse);
        drop(listener);
        assert!(!endpoint.socket_path().exists(), "removed when closed");
        // A socket left by a crash doesn't stop the next start.
        let stale = std::os::unix::net::UnixListener::bind(endpoint.socket_path()).unwrap();
        drop(stale);
        let _listener = endpoint.listen().await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn connecting_without_the_app_fails_fast() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::new(dir.path());
        assert!(endpoint.connect().await.is_err());
    }
}
