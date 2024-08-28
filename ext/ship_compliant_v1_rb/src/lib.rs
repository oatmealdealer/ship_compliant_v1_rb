use std::{
    fmt::{Debug, Display},
    marker::{Send, Sync},
};

use hyper::header::HeaderValue;
use magnus::{function, prelude::*, Ruby};
use serde::{Deserialize, Serialize};
use ship_compliant_v1_rs::{prelude::Client, ResponseValue};

// Copied directly from reqwest since they don't yet support setting default basic auth in the ClientBuilder
pub fn basic_auth<U, P>(username: U, password: Option<P>) -> HeaderValue
where
    U: Display,
    P: Display,
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

#[derive(Serialize, Deserialize)]
#[serde(
    // transparent,
    // rename_all(deserialize = "camelCase", serialize = "snake_case")
    rename_all = "snake_case"
)]
pub struct Response {
    #[serde(flatten)]
    inner: serde_json::Value,
}

impl Response {
    pub fn new<T>(value: T) -> Result<Self, anyhow::Error>
    where
        T: Serialize,
    {
        Ok(Self {
            inner: serde_json::to_value(value)?,
        })
    }
}

#[magnus::wrap(class = "ShipCompliantV1::Client", free_immediately, size)]
pub struct V1Client {
    inner: Client,
    runtime: tokio::runtime::Runtime,
}

impl V1Client {
    pub fn new(baseurl: String, username: String, password: String) -> Result<Self, magnus::Error> {
        let rt = tokio::runtime::Runtime::new().expect("couldnt create tokio runtime");
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
            runtime: rt,
        })
    }
    fn extract_response<T, E>(
        &self,
        result: Result<ResponseValue<T>, ship_compliant_v1_rs::Error<E>>,
    ) -> Result<Response, anyhow::Error>
    where
        T: Serialize,
        E: Serialize + Debug + Send + Sync + 'static,
        ship_compliant_v1_rs::Error<E>: Display + Debug,
    {
        use ship_compliant_v1_rs::Error;
        match result {
            Err(e) => match e {
                Error::ErrorResponse(resp) => Ok(Response::new(resp.into_inner())?),
                Error::UnexpectedResponse(resp) => {
                    let bytes = self.runtime.block_on(resp.bytes())?;
                    Ok(serde_json::from_slice::<Response>(&bytes)?)
                }
                Error::InvalidResponsePayload(bytes, _) => {
                    Ok(serde_json::from_slice::<Response>(&bytes)?)
                }
                _ => Err(e.into()),
            },
            Ok(resp) => Ok(Response::new(resp.into_inner())?),
        }
    }
    fn call<F, T, E>(&self, f: F) -> Result<magnus::Value, magnus::Error>
    where
        T: Serialize,
        E: Serialize + Debug + Send + Sync + 'static,
        ship_compliant_v1_rs::Error<E>: Display + Debug,
        F: std::future::Future<Output = Result<ResponseValue<T>, ship_compliant_v1_rs::Error<E>>>,
    {
        let ruby = Ruby::get().expect("called from ruby thread");
        match self.extract_response(self.runtime.block_on(f)) {
            Err(e) => Err(magnus::Error::new(
                ruby.exception_standard_error(),
                format!("error: {}", e),
            )),
            Ok(resp) => serde_magnus::serialize(&resp),
        }
    }
    pub fn get_sales_order(&self, sales_order_key: String) -> Result<magnus::Value, magnus::Error> {
        self.call(
            self.inner
                .get_sales_orders_sales_order_key(&sales_order_key),
        )
    }
    pub fn get_sales_tax_rates_by_address(
        &self,
        input: magnus::RHash,
    ) -> Result<magnus::Value, magnus::Error> {
        self.call(
            self.inner
                .post_sales_orders_quote_sales_tax_rate(&serde_magnus::deserialize(input)?),
        )
    }
    pub fn calculate_sales_tax_due_for_order(
        &self,
        input: magnus::RHash,
    ) -> Result<magnus::Value, magnus::Error> {
        self.call(
            self.inner
                .post_sales_orders_quote_sales_tax(&serde_magnus::deserialize(input)?),
        )
    }
    pub fn get_sales_order_tracking(
        &self,
        sales_order_key: String,
        shipment_key: Option<magnus::RArray>,
    ) -> Result<magnus::Value, magnus::Error> {
        let shipment_keys: Option<Vec<String>> = match shipment_key {
            None => None,
            Some(keys) => Some(keys.to_vec()?),
        };
        self.call(
            self.inner.get_sales_orders_sales_order_key_tracking(
                &sales_order_key,
                shipment_keys.as_ref(),
            ),
        )
    }
    pub fn define_ruby_class(ruby: &Ruby, module: &magnus::RModule) -> Result<(), magnus::Error> {
        let class = module.define_class("Client", ruby.class_object())?;
        class.define_singleton_method("new", function!(V1Client::new, 3))?;
        class.define_method(
            "get_sales_order",
            magnus::method!(V1Client::get_sales_order, 1),
        )?;
        class.define_method(
            "get_sales_tax_rates_by_address",
            magnus::method!(V1Client::get_sales_tax_rates_by_address, 1),
        )?;
        class.define_method(
            "calculate_sales_tax_due_for_order",
            magnus::method!(V1Client::calculate_sales_tax_due_for_order, 1),
        )?;
        class.define_method(
            "get_sales_order_tracking",
            magnus::method!(V1Client::get_sales_order_tracking, 2),
        )?;
        Ok(())
    }
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), magnus::Error> {
    let module = ruby.define_module("ShipCompliantV1")?;
    V1Client::define_ruby_class(ruby, &module)?;
    Ok(())
}
