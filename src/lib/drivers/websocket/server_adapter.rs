use std::{
    io::{self, Error as IoError, ErrorKind as IoErrorKind},
    pin::Pin,
    task::{Context, Poll},
};

use axum::extract::ws::{Message, WebSocket};
use bytes::BytesMut;
use futures::{Sink, SinkExt, Stream, StreamExt};
use mavlink_codec::Packet;
use tokio_util::codec::{Decoder, Encoder};

pub struct WebSocketServerAdapter<C> {
    inner: WebSocket,
    codec: C,
    buffer: BytesMut,
}

impl<C> WebSocketServerAdapter<C> {
    pub fn new(websocket: WebSocket, codec: C) -> Self {
        Self {
            inner: websocket,
            codec,
            buffer: BytesMut::new(),
        }
    }
}

impl<C> Sink<Packet> for WebSocketServerAdapter<C>
where
    C: Encoder<Packet, Error = IoError>,
    C: Unpin,
{
    type Error = IoError;

    fn poll_ready(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready_unpin(context)
            .map_err(|error| IoError::new(IoErrorKind::Other, error))
    }

    fn start_send(mut self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        let mut buf = BytesMut::new();
        self.codec.encode(item, &mut buf)?;
        let msg = Message::Binary(buf.freeze());

        self.inner
            .start_send_unpin(msg)
            .map_err(|error| IoError::new(IoErrorKind::Other, error))
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_flush_unpin(context)
            .map_err(|error| IoError::new(IoErrorKind::Other, error))
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_close_unpin(context)
            .map_err(|error| IoError::new(IoErrorKind::Other, error))
    }
}

impl<C> Stream for WebSocketServerAdapter<C>
where
    C: Decoder<Error = IoError>,
    C: Unpin,
{
    type Item = io::Result<C::Item>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            let mut buffer = std::mem::take(&mut self.buffer);
            let decode_result = self.codec.decode(&mut buffer);
            self.buffer = buffer;

            match decode_result {
                Ok(Some(item)) => return Poll::Ready(Some(Ok(item))),
                Ok(None) => { /* need more data */ }
                Err(error) => return Poll::Ready(Some(Err(error))),
            }

            match futures::ready!(self.inner.poll_next_unpin(context)) {
                Some(Ok(Message::Binary(data))) => {
                    self.buffer.extend_from_slice(&data);
                }
                Some(Ok(Message::Close(_) | Message::Ping(_) | Message::Pong(_))) => continue,
                Some(Ok(Message::Text(_))) => {
                    return Poll::Ready(Some(Err(IoError::new(
                        IoErrorKind::InvalidData,
                        "Text frames not supported",
                    ))));
                }
                Some(Err(error)) => {
                    return Poll::Ready(Some(Err(IoError::new(IoErrorKind::Other, error))));
                }
                None => return Poll::Ready(None),
            }
        }
    }
}
