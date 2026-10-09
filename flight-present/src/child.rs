// SPDX-License-Identifier: MIT

use crate::Region;

/// One child of a split and its share. Shares are relative: weights 1 and 2 give a third and two
/// thirds of what is left after the dividers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Child {
    pub weight: u16,
    pub region: Region,
}

impl Child {
    pub fn new(weight: u16, region: Region) -> Self {
        Self { weight, region }
    }
}
