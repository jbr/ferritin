use std::fmt::Arguments;

pub(crate) trait WriteFmt: std::fmt::Write {
    fn write_fmt(&mut self, args: Arguments<'_>) {
        std::fmt::Write::write_fmt(self, args).unwrap()
    }
}

impl WriteFmt for String {}
