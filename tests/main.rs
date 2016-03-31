extern crate ipfs_api as ipfs;

#[test]
fn get_object() {
    let obj = ipfs::object::Object {
        data: b"testing"[..].to_owned(),
        links: vec![],
    };
    let obj = obj.commit().unwrap();

    let obj2 = ipfs::object::get(obj.hash()).unwrap();
    assert_eq!(obj, obj2);
    assert_eq!(&*obj, &*obj2);
    assert_eq!(obj.data, b"testing");
    assert_eq!(obj2.data, b"testing");
}

#[test]
fn publish() {
    let obj = ipfs::object::Object {
        data: b"testing"[..].to_owned(),
        links: vec![],
    };
    let obj = obj.commit().unwrap();
    ipfs::name::publish(&obj).unwrap();
    // TODO: lookup my name.
    let r = ipfs::object::lookup("/ipns/Qme6Q6RCZ7GKmpcuzKDqEtZrisygpTnyneq1N8zchdgWYq").unwrap();
    assert_eq!(*obj.reference(), r);
}
