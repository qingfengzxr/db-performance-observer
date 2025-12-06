use clap::ValueEnum;

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum DbKind {
    Mysql,
    Postgres,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Distribution {
    Uniform,
    Zipf,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum IndexMode {
    On,
    Off,
}

#[derive(Debug)]
pub struct DbConfig {
    pub kind: DbKind,
    pub url: String,
}

pub fn default_url(kind: DbKind) -> String {
    match kind {
        DbKind::Mysql => "mysql://perf:perf@127.0.0.1:3306/perf".to_string(),
        DbKind::Postgres => "postgres://perf:perf@127.0.0.1:5432/perf".to_string(),
    }
}
