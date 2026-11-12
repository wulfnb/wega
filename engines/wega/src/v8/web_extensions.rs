//! Identical native Web API extension order at snapshot build and restore time.
pub(crate) fn extensions(
    url: url::Url,
    directory: Option<std::path::PathBuf>,
) -> Vec<deno_core::Extension> {
    vec![
        deno_webidl::deno_webidl::init(),
        deno_web::deno_web::init(
            deno_web::BlobStore::default_arc(),
            Some(url),
            false,
            Default::default(),
        ),
        deno_net::deno_net::init(None, None),
        deno_fetch::deno_fetch::init(Default::default()),
        deno_websocket::deno_websocket::init(),
        deno_webstorage::deno_webstorage::init(directory),
        deno_crypto::deno_crypto::init(None),
    ]
}
