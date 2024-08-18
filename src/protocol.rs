use std::ops::{Deref, DerefMut};

use mavlink::MAVLinkV2MessageRaw;

#[derive(Debug, Clone)]
pub struct Protocol {
    pub origin: String,
    message: MAVLinkV2MessageRaw,
}

impl Protocol {
    pub fn new(origin: &str, message: MAVLinkV2MessageRaw) -> Self {
        Self {
            origin: origin.to_string(),
            message,
        }
    }
}

impl Deref for Protocol {
    type Target = MAVLinkV2MessageRaw;

    fn deref(&self) -> &Self::Target {
        &self.message
    }
}

impl DerefMut for Protocol {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.message
    }
}
