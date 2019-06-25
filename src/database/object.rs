use crate::database::ParsedObject;
use crypto::digest::Digest;
use crypto::sha1::Sha1;

pub trait Object {
    fn r#type(&self) -> String;
    fn to_string(&self) -> Vec<u8>;

    fn parse(s: &[u8]) -> ParsedObject;

    fn get_oid(&self) -> String {
        let mut hasher = Sha1::new();
        hasher.input(&self.get_content());
        hasher.result_str()
    }

    fn get_content(&self) -> Vec<u8> {
        // TODO: need to do something to force ASCII encoding?
        let string = self.to_string();
        let mut content: Vec<u8> = self.r#type().as_bytes().to_vec();

        content.push(0x20);
        content.extend_from_slice(format!("{}", string.len()).as_bytes());
        content.push(0x0);
        content.extend_from_slice(&string);

        content
    }
}
