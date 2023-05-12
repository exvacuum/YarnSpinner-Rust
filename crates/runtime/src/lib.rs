extern crate alloc;

mod dialogue;
mod dialogue_option;
mod handlers;
mod line;
mod pluralization;
mod string_newtype;
mod variable_storage;
mod virtual_machine;

pub(crate) use string_newtype::string_newtype;

pub mod prelude {
    pub(crate) use crate::virtual_machine::*;
    pub use crate::{
        dialogue::*, dialogue_option::*, handlers::*, line::*, pluralization::*,
        variable_storage::*,
    };
}