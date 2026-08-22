pub(crate) mod commands;
mod firewall;
mod ftp;
mod manager;
mod models;
mod password;
mod sftp;
mod tftp;

pub(crate) use manager::shutdown_runtime;
pub(crate) use manager::FileServiceManager;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileServiceProtocol {
    Tftp,
    Ftp,
    Sftp,
}

impl FileServiceProtocol {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "tftp" => Ok(Self::Tftp),
            "ftp" => Ok(Self::Ftp),
            "sftp" => Ok(Self::Sftp),
            _ => Err(format!("unsupported file service protocol '{value}'")),
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Tftp => "tftp",
            Self::Ftp => "ftp",
            Self::Sftp => "sftp",
        }
    }
}

impl std::fmt::Display for FileServiceProtocol {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}
