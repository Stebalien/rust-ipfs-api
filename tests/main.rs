#![feature(question_mark)]

extern crate ipfs_api as ipfs;

macro_rules! unwrap {
    (@ $b:block) => {
            (|| -> ::std::result::Result<(), Box<::std::error::Error>> {
                $b
                Ok(())
            })().unwrap()
    };
    ($($tok:tt)*) => {
        unwrap!(@ {$($tok)*});
    };
}

#[test]
fn get_object() {
    unwrap! {
        let obj = ipfs::object::Object {
            data: b"testing"[..].to_owned(),
            links: vec![],
        };
        let obj = obj.commit()?;

        let obj2 = ipfs::object::get(obj.hash())?;
        assert_eq!(obj, obj2);
        assert_eq!(&*obj, &*obj2);
        assert_eq!(obj.data, b"testing");
        assert_eq!(obj2.data, b"testing");
    }
}
