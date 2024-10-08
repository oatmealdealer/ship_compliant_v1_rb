use hyper::header::HeaderValue;
use inflections::Inflect;
use magnus::{function, prelude::*, RString, Ruby, StaticSymbol};
use serde::Serialize;
use ship_compliant_v1_rs::{prelude::Client, ResponseValue};
use std::{
    fmt::{Debug, Display},
    marker::{Send, Sync},
};

pub fn snake_case_hash_keys_mut(resp: &magnus::Value) -> Result<magnus::Value, magnus::Error> {
    let ruby_hash = magnus::RHash::from_value(*resp).expect("expected Hash");

    ruby_hash.block_call::<_, _, magnus::StaticSymbol, magnus::RHash>(
        "deep_transform_keys!",
        (),
        |_, values, _| -> StaticSymbol {
            StaticSymbol::new(
                RString::from_value(values.get(0).unwrap().funcall_public("to_s", ()).unwrap())
                    .unwrap()
                    .to_string()
                    .unwrap()
                    .to_snake_case(),
            )
        },
    )?;
    Ok(ruby_hash.as_value())
}

pub fn camel_case_hash_keys(resp: &magnus::Value) -> Result<magnus::Value, magnus::Error> {
    let mut ruby_hash = magnus::RHash::from_value(*resp).expect("expected Hash");

    ruby_hash = ruby_hash.block_call::<_, _, magnus::StaticSymbol, magnus::RHash>(
        "deep_transform_keys",
        (),
        |_, values, _| -> StaticSymbol {
            StaticSymbol::new(
                RString::from_value(values.get(0).unwrap().funcall_public("to_s", ()).unwrap())
                    .unwrap()
                    .to_string()
                    .unwrap()
                    .to_camel_case(),
            )
        },
    )?;
    Ok(ruby_hash.as_value())
}

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
    ) -> Result<magnus::Value, magnus::Error>
    where
        T: Serialize,
        E: Serialize + Debug + Send + Sync,
        ship_compliant_v1_rs::Error<E>: Display + Debug,
    {
        use ship_compliant_v1_rs::Error;
        let ruby = Ruby::get().expect("called from ruby thread");
        match result {
            Err(e) => match e {
                Error::ErrorResponse(resp) => serde_magnus::serialize(&resp.into_inner()),
                Error::UnexpectedResponse(resp) => {
                    let bytes = self.runtime.block_on(resp.bytes()).map_err(move |e| {
                        magnus::Error::new(ruby.exception_standard_error(), format!("error: {}", e))
                    })?;
                    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
                    let serializer = serde_magnus::Serializer;
                    Ok(serde_transcode::transcode(&mut deserializer, serializer)?)
                }
                Error::InvalidResponsePayload(bytes, _) => {
                    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
                    let serializer = serde_magnus::Serializer;
                    Ok(serde_transcode::transcode(&mut deserializer, serializer)?)
                }
                _ => Err(magnus::Error::new(
                    ruby.exception_standard_error(),
                    format!("error: {}", e),
                )),
            },
            Ok(resp) => serde_magnus::serialize(&resp.into_inner()),
        }
    }
    fn call<F, T, E>(&self, f: F) -> Result<magnus::Value, magnus::Error>
    where
        T: Serialize,
        E: Serialize + Debug + Send + Sync,
        ship_compliant_v1_rs::Error<E>: Display + Debug,
        F: std::future::Future<Output = Result<ResponseValue<T>, ship_compliant_v1_rs::Error<E>>>,
    {
        let ruby = Ruby::get().expect("called from ruby thread");
        match self.extract_response(self.runtime.block_on(f)) {
            Err(e) => Err(magnus::Error::new(
                ruby.exception_standard_error(),
                format!("error: {}", e),
            )),
            Ok(resp) => snake_case_hash_keys_mut(&resp),
        }
    }
    pub fn get_sales_order(&self, sales_order_key: String) -> Result<magnus::Value, magnus::Error> {
        self.call(
            self.inner
                .get_sales_orders_sales_order_key(&sales_order_key),
        )
    }
    pub fn void_sales_order(
        &self,
        sales_order_key: String,
    ) -> Result<magnus::Value, magnus::Error> {
        self.call(
            self.inner
                .delete_sales_orders_sales_order_key(&sales_order_key),
        )
    }
    pub fn check_compliance_of_sales_order(
        &self,
        input: magnus::RHash,
    ) -> Result<magnus::Value, magnus::Error> {
        self.call(
            self.inner
                .post_sales_orders_check_compliance(&serde_magnus::deserialize(
                    camel_case_hash_keys(&input.as_value())?,
                )?),
        )
    }
    pub fn commit_sales_order(&self, input: magnus::RHash) -> Result<magnus::Value, magnus::Error> {
        self.call(
            self.inner
                .post_sales_orders_commit(&serde_magnus::deserialize(camel_case_hash_keys(
                    &input.as_value(),
                )?)?),
        )
    }
    pub fn persist_sales_order(
        &self,
        input: magnus::RHash,
    ) -> Result<magnus::Value, magnus::Error> {
        self.call(
            self.inner
                .post_sales_orders(&serde_magnus::deserialize(camel_case_hash_keys(
                    &input.as_value(),
                )?)?),
        )
    }
    pub fn add_update_product(&self, input: magnus::RHash) -> Result<magnus::Value, magnus::Error> {
        self.call(
            self.inner
                .post_products(&serde_magnus::deserialize(camel_case_hash_keys(
                    &input.as_value(),
                )?)?),
        )
    }
    pub fn add_update_brand(&self, input: magnus::RHash) -> Result<magnus::Value, magnus::Error> {
        self.call(
            self.inner
                .post_brands(&serde_magnus::deserialize(camel_case_hash_keys(
                    &input.as_value(),
                )?)?),
        )
    }
    pub fn get_sales_tax_rates_by_address(
        &self,
        input: magnus::RHash,
    ) -> Result<magnus::Value, magnus::Error> {
        self.call(
            self.inner
                .post_sales_orders_quote_sales_tax_rate(&serde_magnus::deserialize(
                    camel_case_hash_keys(&input.as_value())?,
                )?),
        )
    }
    pub fn calculate_sales_tax_due_for_order(
        &self,
        input: magnus::RHash,
    ) -> Result<magnus::Value, magnus::Error> {
        self.call(
            self.inner
                .post_sales_orders_quote_sales_tax(&serde_magnus::deserialize(
                    camel_case_hash_keys(&input.as_value())?,
                )?),
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
            "check_compliance_of_sales_order",
            magnus::method!(V1Client::check_compliance_of_sales_order, 1),
        )?;
        class.define_method(
            "commit_sales_order",
            magnus::method!(V1Client::commit_sales_order, 1),
        )?;
        class.define_method(
            "persist_sales_order",
            magnus::method!(V1Client::persist_sales_order, 1),
        )?;
        class.define_method(
            "void_sales_order",
            magnus::method!(V1Client::void_sales_order, 1),
        )?;
        class.define_method(
            "add_update_product",
            magnus::method!(V1Client::add_update_product, 1),
        )?;
        class.define_method(
            "add_update_brand",
            magnus::method!(V1Client::add_update_brand, 1),
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
