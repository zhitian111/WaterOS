#![no_std]

use core::fmt;
// Console Trait 包括向控制台输出和从控制台读入的特性
pub trait Console: fmt::Write + Default {}
