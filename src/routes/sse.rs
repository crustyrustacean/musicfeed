// src/routes/sse.rs

use actix_sse::{Event, Sse};
use actix_web::Responder;
use datastar::DatastarEvent;

pub(crate) fn datastar_sse<I>(events: I) -> impl Responder
where
    I: IntoIterator<Item = DatastarEvent>,
    I::IntoIter: 'static,
{
    Sse::from_infallible_stream(tokio_stream::iter(events.into_iter().map(Event::from)))
}
