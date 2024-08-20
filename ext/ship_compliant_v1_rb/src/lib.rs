use hyper::header::HeaderValue;
use magnus::{function, prelude::*, Error, Ruby};
use ship_compliant_v1_rs::prelude::Client;


// Copied directly from reqwest since they don't yet support setting default basic auth in the ClientBuilder
pub fn basic_auth<U, P>(username: U, password: Option<P>) -> HeaderValue
where
    U: std::fmt::Display,
    P: std::fmt::Display,
{
    use base64::prelude::BASE64_STANDARD;
    use base64::write::EncoderWriter;
    use std::io::Write;

    let mut buf = b"Basic ".to_vec();
    {
        let mut encoder = EncoderWriter::new(&mut buf, &BASE64_STANDARD);
        let _ = write!(encoder, "{username}:");
        if let Some(password) = password {
            let _ = write!(encoder, "{password}");
        }
    }
    let mut header = HeaderValue::from_bytes(&buf).expect("base64 is always valid HeaderValue");
    header.set_sensitive(true);
    header
}

#[magnus::wrap(class = "ShipCompliantV1::Client")]
pub struct V1Client {
    inner: Client,
}

impl V1Client {
    pub fn new(baseurl: String, username: String, password: String) -> Result<Self, magnus::Error> {
        let mut headers = reqwest::header::HeaderMap::with_capacity(1);
        headers.insert(
            reqwest::header::AUTHORIZATION,
            basic_auth(username, Some(password)),
        );
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("unable to build client");
        Ok(Self {
            inner: Client::new_with_client(&baseurl, client),
        })
    }
    pub async fn get_sales_order(
        &self,
        ruby: Option<&Ruby>,
        sales_order_key: String,
    ) -> Result<magnus::Value, magnus::Error> {
        match self
            .inner
            .get_sales_orders_sales_order_key(Some(&sales_order_key))
            .await
        {
            Ok(resp) => Ok(serde_magnus::serialize(
                resp.sales_order
                    .as_ref()
                    .expect("missing sales order for successful response"),
            )
            .expect("couldn't serialize response")),
            Err(e) => Err(magnus::Error::new(
                ruby.expect("must be called from Ruby thread")
                    .exception_standard_error(),
                format!("error: {}", e.to_string()),
            )),
        }
    }
    pub fn define_ruby_class(ruby: &Ruby, module: &magnus::RModule) -> Result<(), magnus::Error> {
        let class = module.define_class("Client", ruby.class_object())?;
        class.define_singleton_method("new", function!(V1Client::new, 3))?;

        Ok(())
    }
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("ShipCompliantV1")?;
    // let error_class = module.define_error("Error", ruby.exception_standard_error())?;

    Ok(())
}
