use std::{
    io::{self, Error as IoError, ErrorKind as IoErrorKind},
    pin::Pin,
    task::{Context, Poll},
};

use bytes::BytesMut;
use futures::{Sink, Stream};
use mavlink_codec::Packet;
use tokio_util::codec::Decoder;
use zenoh::{
    Wait,
    handlers::FifoChannelHandler,
    pubsub::{Publisher, Subscriber},
    sample::Sample,
};

/// Adapts raw Zenoh samples to the standard MAVLink `Sink` and `Stream` interfaces.
pub struct ZenohRawAdapter<C> {
    writer: ZenohRawWriter,
    reader: ZenohRawReader<C>,
}

/// Sends raw MAVLink packets over a Zenoh publisher.
pub struct ZenohRawWriter {
    publisher: Publisher<'static>,
}

/// Receives raw Zenoh samples and decodes MAVLink packets.
pub struct ZenohRawReader<C> {
    _subscriber: Subscriber<FifoChannelHandler<Sample>>,
    samples: Pin<Box<dyn Stream<Item = Sample> + Send>>,
    codec: C,
    buffer: BytesMut,
    end_of_stream: bool,
}

impl<C> ZenohRawAdapter<C> {
    /// Creates an adapter from a Zenoh subscriber, publisher, and packet codec.
    pub fn new(
        subscriber: Subscriber<FifoChannelHandler<Sample>>,
        publisher: Publisher<'static>,
        codec: C,
    ) -> Self {
        let samples = Box::pin(subscriber.handler().clone().into_stream());

        Self {
            writer: ZenohRawWriter { publisher },
            reader: ZenohRawReader {
                _subscriber: subscriber,
                samples,
                codec,
                buffer: BytesMut::new(),
                end_of_stream: false,
            },
        }
    }

    /// Splits the adapter into independent writer and reader halves without a shared lock.
    pub fn split(self) -> (ZenohRawWriter, ZenohRawReader<C>) {
        (self.writer, self.reader)
    }
}

impl Sink<Packet> for ZenohRawWriter {
    type Error = IoError;

    fn poll_ready(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        self.publisher
            .put(item.bytes().clone())
            .wait()
            .map_err(|error| IoError::new(IoErrorKind::ConnectionReset, error))?;
        Ok(())
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl<C> Stream for ZenohRawReader<C>
where
    C: Decoder<Error = IoError>,
    C: Unpin,
{
    type Item = io::Result<C::Item>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            let mut buffer = std::mem::take(&mut self.buffer);
            let decode_result = if self.end_of_stream {
                self.codec.decode_eof(&mut buffer)
            } else {
                self.codec.decode(&mut buffer)
            };
            self.buffer = buffer;

            match decode_result {
                Ok(Some(item)) => return Poll::Ready(Some(Ok(item))),
                Ok(None) if self.end_of_stream => return Poll::Ready(None),
                Ok(None) => { /* need more data */ }
                Err(error) => return Poll::Ready(Some(Err(error))),
            }

            match futures::ready!(self.samples.as_mut().poll_next(context)) {
                Some(sample) => {
                    self.buffer.reserve(sample.payload().len());
                    for slice in sample.payload().slices() {
                        self.buffer.extend_from_slice(slice);
                    }
                }
                None => self.end_of_stream = true,
            }
        }
    }
}
