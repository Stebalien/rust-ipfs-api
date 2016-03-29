use std::sync::RwLock;
use std::io::{self, Read};

use multipart::client::Multipart;
use url::{self, Url, UrlParser};
use hyper::{self, net};
use hyper::client::pool::Pool;
use hyper::method::Method;
use hyper::client::request::Request;
use protobuf::{self, MessageStatic};

use serde;
use serde_json;

const API_VERSION: &'static str = "v0";


#[derive(Debug, Deserialize)]
pub struct IpfsError {
    #[serde(rename="Message")]
    pub message: String,
    #[serde(rename="Code")]
    pub code: u32,
}

pub mod ipfs_error {
    pub const NOT_PINNED: &'static str = "not pinned";
    pub const INVALID_REF: &'static str = "invalid ipfs ref path";
}


thread_local! {
    static CONN_POOL: Pool<net::DefaultConnector> = Pool::new(Default::default())
}

lazy_static! {
    static ref IPFS_BASE: RwLock<Url> = RwLock::new(Url {
        scheme: String::from("http"),
        scheme_data: url::SchemeData::Relative(url::RelativeSchemeData {
            host: url::Host::Domain(String::from("127.0.0.1")),
            port: Some(5001),
            username: String::new(),
            default_port: Some(80),
            password: None,
            path: vec![String::from("api"), String::from(API_VERSION), String::new()],
        }),
        query: None,
        fragment: None,
    });
}

pub trait Encoding<T> {
    const ENCODING: Option<&'static str>;
    fn parse(reader: &mut Read) -> io::Result<T>;
}

pub struct Json;
pub struct Ignore;
pub struct Protobuf;

impl Encoding<()> for Ignore {
    const ENCODING: Option<&'static str> = None;
    fn parse(_: &mut Read) -> io::Result<()> {
        Ok(())
    }
}

impl<T: serde::Deserialize> Encoding<T> for Json {
    const ENCODING: Option<&'static str> = Some("json");
    fn parse(r: &mut Read) -> io::Result<T> {
        use serde_json::error::Error::Io;
        serde_json::from_reader(r).map_err(|e| {
            match e {
                Io(e) => io::Error::new(io::ErrorKind::InvalidData, e),
                e => io::Error::new(io::ErrorKind::InvalidData, e),
            }
        })
    }
}

impl<T: MessageStatic> Encoding<T> for Protobuf {
    const ENCODING: Option<&'static str> = Some("protobuf");

    fn parse(r: &mut Read) -> io::Result<T> {
        use protobuf::ProtobufError::*;
        protobuf::parse_from_reader::<T>(r).map_err(|e| {
            match e {
                IoError(e) => e,
                WireError(e) => io::Error::new(io::ErrorKind::InvalidData, e),
            }
        })
    }
}


/// Set the IPFS API endpoint
pub fn set_api_endpoint(url: Url) {
    *IPFS_BASE.write().unwrap() = url;
}

/// Get the IPFS API endpoint
pub fn get_api_endpoint() -> Url {
    IPFS_BASE.read().unwrap().clone()
}


fn request(method: Method, url: Url) -> hyper::Result<Request<net::Fresh>> {
    CONN_POOL.with(|pool| Request::with_connector(method, url, pool))
}

// Panics if method is not a valid URL path.
fn make_url(method: &str, args: &[(&str, &str)], encoding: Option<&str>) -> Url {
    let mut url = match UrlParser::new().base_url(&IPFS_BASE.read().unwrap()).parse(method) {
        Ok(v) => v,
        Err(_) => panic!("invalid url"),
    };
    url.set_query_from_pairs(encoding.map(|e|("encoding", e)).iter().chain(args));
    url
}

fn handle_error<P, T>(mut response: hyper::client::Response) -> io::Result<T>
    where P: Encoding<T>
{
    if response.status.is_success() {
        P::parse(&mut response)
    } else {
        let result: IpfsError = Json::parse(&mut response)?;
        return Err(io::Error::new(io::ErrorKind::Other, result.message))
    }
}

pub fn get<P, T>(method: &str, args: &[(&str, &str)]) -> io::Result<T>
    where P: Encoding<T>
{
    let resp = match request(Method::Get, make_url(method, args, <P as Encoding<T>>::ENCODING)).and_then(|r| r.start()).and_then(|r| r.send()) {
        Ok(v) => v,
        Err(hyper::Error::Io(e)) => return Err(e),
        Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e)),
    };
    handle_error::<P, T>(resp)
}

pub fn post<P, T>(method: &str, args: &[(&str, &str)]) -> io::Result<T> 
    where P: Encoding<T>
    {
    let resp = match request(Method::Post, make_url(method, args, <P as Encoding<T>>::ENCODING)).and_then(|r| r.start()).and_then(|r| r.send()) {
        Ok(v) => v,
        Err(hyper::Error::Io(e)) => return Err(e),
        Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e)),
    };
    handle_error::<P, T>(resp)
}

pub fn post_data<P, T>(method: &str,
            args: &[(&str, &str)],
            data: &[u8])
            -> io::Result<T>
    where P: Encoding<T>
    {
    let resp = match request(Method::Post, make_url(method, args, <P as Encoding<T>>::ENCODING))
                         .and_then(|mut r| {
                             r.headers_mut().set(hyper::header::Connection::close());
                             Multipart::from_request(r)
                         })
                         .and_then(|mut r| {
                             // XXX: Why does rust insist that this must be used?
                             let _ = r.write_stream("data", &mut &*data, None, None);
                             r.send()
                         }) {
        Ok(v) => v,
        Err(hyper::Error::Io(e)) => return Err(e),
        Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e)),
    };
    handle_error::<P, T>(resp)
}
