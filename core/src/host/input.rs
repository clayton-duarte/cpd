//! The keyboard of a command that owns its terminal (`session run`'s "Press Enter"): the process's stdin, or the input a
//! test scoped on this thread ([`scope`]), as [`crate::ports::Ports::scope`] scopes the ports. A test that runs the
//! wrapper in a thread of its own gives it an input it controls, so the wrapper never blocks on a terminal.

use std::cell::RefCell;
use std::io::{self, BufRead};

thread_local! {
    /// The input of the innermost [`scope`] on this thread.
    static SCOPED: RefCell<Option<Box<dyn BufRead>>> = const { RefCell::new(None) };
}

/// Runs `f` with `input` as what [`read_line`] reads on this thread, then puts the previous input back, also when `f`
/// panics. A thread `f` spawns does not see it.
pub fn scope<R>(input: Box<dyn BufRead>, f: impl FnOnce() -> R) -> R {
    /// Puts the previous input back on drop, so a panic of `f` restores it too.
    struct Restore(Option<Box<dyn BufRead>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            SCOPED.with(|cell| *cell.borrow_mut() = previous);
        }
    }
    let previous = SCOPED.with(|cell| cell.borrow_mut().replace(input));
    let _restore = Restore(previous);
    f()
}

/// One line of the input scoped on this thread, else of the process's stdin: the bytes read, `0` at the end of input.
pub fn read_line(line: &mut String) -> io::Result<usize> {
    SCOPED.with(|cell| match cell.borrow_mut().as_mut() {
        Some(input) => input.read_line(line),
        None => io::stdin().lock().read_line(line),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scoped_input_is_read_line_by_line_and_the_end_is_zero() {
        scope(Box::new(&b"one\ntwo\n"[..]), || {
            let mut line = String::new();
            assert_eq!(read_line(&mut line).unwrap(), 4);
            assert_eq!(line, "one\n");
            line.clear();
            assert_eq!(read_line(&mut line).unwrap(), 4);
            assert_eq!(line, "two\n");
            line.clear();
            assert_eq!(read_line(&mut line).unwrap(), 0);
        });
    }

    #[test]
    fn scopes_nest_and_the_outer_input_comes_back_also_after_a_panic() {
        scope(Box::new(&b"outer\n"[..]), || {
            let inner = std::panic::catch_unwind(|| {
                scope(Box::new(&b"inner\n"[..]), || -> () { panic!("inner") })
            });
            assert!(inner.is_err());
            let mut line = String::new();
            read_line(&mut line).unwrap();
            assert_eq!(line, "outer\n");
        });
    }
}
