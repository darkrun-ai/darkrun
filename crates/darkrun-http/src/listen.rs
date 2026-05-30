//! A connection-capping [`axum::serve::Listener`] wrapper.
//!
//! Wraps a [`tokio::net::TcpListener`] and bounds the number of concurrently
//! live connections to a fixed ceiling. Each accepted socket carries an owned
//! semaphore permit; the permit (and thus the slot) is released when the socket
//! is dropped at the end of its connection. This is the axum-0.7 way to apply
//! a max-connections cap.
//!
//! When the cap is reached, `accept` parks until a slot frees up, applying
//! backpressure at the TCP-accept layer rather than ripping sockets out from
//! under in-flight requests.

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// A [`TcpListener`] that caps concurrent connections via a semaphore.
pub struct CappedListener {
    listener: TcpListener,
    permits: Arc<Semaphore>,
}

impl CappedListener {
    /// Wrap `listener`, allowing at most `max_connections` live connections.
    pub fn new(listener: TcpListener, max_connections: usize) -> Self {
        Self {
            listener,
            permits: Arc::new(Semaphore::new(max_connections.max(1))),
        }
    }
}

impl axum::serve::Listener for CappedListener {
    type Io = CappedStream;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            // Park until a connection slot is available. The semaphore is never
            // closed, so acquisition cannot fail.
            let permit = Arc::clone(&self.permits)
                .acquire_owned()
                .await
                .expect("connection semaphore closed");
            match self.listener.accept().await {
                Ok((stream, addr)) => {
                    return (
                        CappedStream {
                            stream,
                            _permit: permit,
                        },
                        addr,
                    );
                }
                // Transient accept error: drop the permit and retry, matching
                // the resilience of axum's built-in TCP listener.
                Err(_) => {
                    drop(permit);
                    continue;
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.listener.local_addr()
    }
}

/// The peer socket address of a connection, as a local newtype.
///
/// axum's blanket `Connected<IncomingStream<'_, TcpListener>> for SocketAddr`
/// is confined to axum's crate by the orphan rule, so for our custom
/// [`CappedListener`] we extract the peer address into this local wrapper
/// instead and have handlers read `ConnectInfo<PeerAddr>`.
#[derive(Debug, Clone, Copy)]
pub struct PeerAddr(pub SocketAddr);

impl axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, CappedListener>>
    for PeerAddr
{
    fn connect_info(stream: axum::serve::IncomingStream<'_, CappedListener>) -> Self {
        PeerAddr(*stream.remote_addr())
    }
}

/// A [`TcpStream`] that holds its connection-slot permit until dropped.
pub struct CappedStream {
    stream: TcpStream,
    _permit: OwnedSemaphorePermit,
}

impl AsyncRead for CappedStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for CappedStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.stream.is_write_vectored()
    }
}
