use std::fmt;

pub struct DisplayPrefix<'a, T: fmt::Display> {
    prefix: &'a str,
    data: T
}

impl<'a, T: fmt::Display> fmt::Display for DisplayPrefix<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let text = format!("{}", self.data);
        let mut first_line = true;
        for line in text.lines() {
            if !first_line {
                try!(f.write_str("\n"));
            }
            first_line = false;
            try!(write!(f, "{}{}", self.prefix, line));
        }
        Ok(())
    }
}

pub fn with_prefix<'a, T>(prefix: &'a str, data: T) -> DisplayPrefix<'a, T>
        where T: fmt::Display {
    DisplayPrefix {
        prefix: prefix,
        data: data
    }
}
