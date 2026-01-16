use clap::ValueEnum;

/// Controls the verbosity level of documentation display
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[derive(Default)]
pub(crate) enum Verbosity {
    Minimal,
    Brief,
    #[default]
    Full,
}

impl Verbosity {
    pub(crate) fn is_full(self) -> bool {
        matches!(self, Self::Full)
    }
}

