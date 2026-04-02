#![no_std]
#[derive(Default)]
pub struct DummyConsoleHandle;
impl core::fmt::Write for DummyConsoleHandle {
    #[allow(unused)]
    #[allow(unused_variables)]
    fn write_str(&mut self, s : &str) -> core::fmt::Result { unimplemented!() }
}
impl api_v0::Console for DummyConsoleHandle {}
