use axum::{
    extract::{rejection::RawFormRejection, FromRequest, RawForm, Request},
    response::{IntoResponse, Response},
    Error, RequestExt,
};
use http::StatusCode;
use serde::de::DeserializeOwned;
use std::fmt;

/// Extractor that deserializes `application/x-www-form-urlencoded` requests
/// into some type.
///
/// `T` is expected to implement [`serde::Deserialize`].
///
/// # Differences from `axum::extract::Form`
///
/// This extractor uses [`serde_html_form`] under-the-hood which supports multi-value items. These
/// are sent by multiple `<input>` attributes of the same name (e.g. checkboxes) and `<select>`s
/// with the `multiple` attribute. Those values can be collected into a `Vec` or other sequential
/// container.
///
/// # Example
///
/// ```rust,no_run
/// use axum_extra::extract::Form;
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Payload {
///     #[serde(rename = "value")]
///     values: Vec<String>,
/// }
///
/// async fn accept_form(Form(payload): Form<Payload>) {
///     // ...
/// }
/// ```
///
/// [`serde_html_form`]: https://crates.io/crates/serde_html_form
#[derive(Debug, Clone, Copy, Default)]
#[cfg(feature = "form")]
pub struct Form<T>(pub T);

axum_core::__impl_deref!(Form);

impl<T, S> FromRequest<S> for Form<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = FormRejection;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let RawForm(bytes) = req
            .extract()
            .await
            .map_err(FormRejection::RawFormRejection)?;

        let deserializer = serde_html_form::Deserializer::new(form_urlencoded::parse(&bytes));

        serde_path_to_error::deserialize::<_, T>(deserializer)
            .map(Self)
            .map_err(|err| FormRejection::FailedToDeserializeForm(Error::new(err)))
    }
}

/// Rejection used for [`Form`].
///
/// Contains one variant for each way the [`Form`] extractor can fail.
#[derive(Debug)]
#[non_exhaustive]
#[cfg(feature = "form")]
pub enum FormRejection {
    #[allow(missing_docs)]
    RawFormRejection(RawFormRejection),
    #[allow(missing_docs)]
    FailedToDeserializeForm(Error),
}

impl FormRejection {
    /// Get the status code used for this rejection.
    pub fn status(&self) -> StatusCode {
        // Make sure to keep this in sync with `IntoResponse` impl.
        match self {
            Self::RawFormRejection(inner) => inner.status(),
            Self::FailedToDeserializeForm(_) => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for FormRejection {
    fn into_response(self) -> Response {
        let status = self.status();
        match self {
            Self::RawFormRejection(inner) => inner.into_response(),
            Self::FailedToDeserializeForm(inner) => {
                let body = format!("Failed to deserialize form: {inner}");
                axum_core::__log_rejection!(
                    rejection_type = Self,
                    body_text = body,
                    status = status,
                );
                (status, body).into_response()
            }
        }
    }
}

impl fmt::Display for FormRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RawFormRejection(inner) => inner.fmt(f),
            Self::FailedToDeserializeForm(inner) => inner.fmt(f),
        }
    }
}

impl std::error::Error for FormRejection {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RawFormRejection(inner) => Some(inner),
            Self::FailedToDeserializeForm(inner) => Some(inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use axum::routing::{on, post, MethodFilter};
    use axum::Router;
    use http::header::CONTENT_TYPE;
    use mime::APPLICATION_WWW_FORM_URLENCODED;
    use serde::Deserialize;

    #[tokio::test]
    async fn supports_multiple_values() {
        #[derive(Deserialize)]
        struct Data {
            #[serde(rename = "value")]
            values: Vec<String>,
        }

        let app = Router::new().route(
            "/",
            post(|Form(data): Form<Data>| async move { data.values.join(",") }),
        );

        let client = TestClient::new(app);

        let res = client
            .post("/")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body("value=one&value=two")
            .await;

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.text().await, "one,two");
    }

    #[tokio::test]
    async fn deserialize_error_status_codes() {
        #[allow(dead_code)]
        #[derive(Deserialize)]
        struct Payload {
            a: i32,
        }

        let app = Router::new().route(
            "/",
            on(
                MethodFilter::GET.or(MethodFilter::POST),
                |_: Form<Payload>| async {},
            ),
        );

        let client = TestClient::new(app);

        let res = client.get("/?a=false").await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            res.text().await,
            "Failed to deserialize form: a: invalid digit found in string"
        );

        let res = client
            .post("/")
            .header(CONTENT_TYPE, APPLICATION_WWW_FORM_URLENCODED.as_ref())
            .body("a=false")
            .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            res.text().await,
            "Failed to deserialize form: a: invalid digit found in string"
        );
    }
}
