use std::fmt::Write;
use std::fmt::{self, Formatter};

pub(crate) struct Indent<'a>(&'a str, usize);

impl<'a> Indent<'a> {
    pub(crate) fn new(string: &'a str, indentation: usize) -> Self {
        Self(string, indentation)
    }
}
impl std::fmt::Display for Indent<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for line in self.0.lines() {
            for _ in 0..self.1 {
                f.write_char(' ')?;
            }
            f.write_str(line)?;
            f.write_char('\n')?;
        }

        Ok(())
    }
}
