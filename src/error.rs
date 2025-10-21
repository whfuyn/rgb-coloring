// TODO

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("failed to read contract: `{0}`")]
    ReadContract(String),
    #[error("failed to transfer: `{0}`")]
    Transfer(String),
}

impl Error {
    pub fn read_contract(msg: String) -> Self {
        Error::ReadContract(msg)
    }

    pub fn transfer(msg: String) -> Self {
        Error::Transfer(msg)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
