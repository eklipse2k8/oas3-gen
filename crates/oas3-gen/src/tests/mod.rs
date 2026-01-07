pub mod common;
#[cfg(feature = "eventsource")]
#[cfg(test)]
mod event_stream;
#[cfg(test)]
mod intersection_union;
#[cfg(test)]
mod petstore;
#[cfg(test)]
mod union_serde;
