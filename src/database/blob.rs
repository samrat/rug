use crate::database::object::Object;
use crate::database::ParsedObject;

#[derive(Debug)]
pub struct Blob {
    pub data: Vec<u8>,
}

impl Blob {
    pub fn new(data: &[u8]) -> Blob {
        Blob {
            data: data.to_vec(),
        }
    }
}

impl Object for Blob {
    fn r#type(&self) -> String {
        "blob".to_string()
    }

    fn to_string(&self) -> Vec<u8> {
        self.data.clone()
    }

    fn parse(s: &[u8]) -> ParsedObject {
        ParsedObject::Blob(Blob::new(s))
    }
}
