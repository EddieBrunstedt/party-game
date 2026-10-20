//! UI strings through Fluent (DESIGN.md §4.6). English only for now; every string a player
//! reads comes from `locales/en/main.ftl` by key, so adding a language is a new file.

use std::sync::OnceLock;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};

const EN: &str = include_str!("../locales/en/main.ftl");

fn bundle() -> &'static FluentBundle<FluentResource> {
    static BUNDLE: OnceLock<FluentBundle<FluentResource>> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        let resource = FluentResource::try_new(EN.to_string())
            .unwrap_or_else(|(_, errors)| panic!("main.ftl has syntax errors: {errors:?}"));
        let mut bundle =
            FluentBundle::new_concurrent(vec!["en".parse().expect("valid language id")]);
        // Unicode isolation marks show up as boxes in the UI font.
        bundle.set_use_isolating(false);
        bundle
            .add_resource(resource)
            .expect("main.ftl defines each key once");
        bundle
    })
}

/// The string for `key`, with `args` filled in. A missing key shows as the key itself, so
/// it's obvious on screen rather than silently blank.
pub fn tr_args(key: &str, args: &[(&str, FluentValue)]) -> String {
    let bundle = bundle();
    let Some(pattern) = bundle.get_message(key).and_then(|m| m.value()) else {
        return key.to_string();
    };
    let mut fluent_args = FluentArgs::new();
    for (name, value) in args {
        fluent_args.set(*name, value.clone());
    }
    let mut errors = Vec::new();
    bundle
        .format_pattern(pattern, Some(&fluent_args), &mut errors)
        .into_owned()
}

/// `tr!("key")` or `tr!("key", name = value, ...)`.
#[macro_export]
macro_rules! tr {
    ($key:expr) => {
        $crate::i18n::tr_args($key, &[])
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::tr_args(
            $key,
            &[$((stringify!($name), fluent_bundle::FluentValue::from($value))),+],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_resolves_without_errors() {
        let bundle = bundle();
        let resource = FluentResource::try_new(EN.to_string()).unwrap();
        for entry in resource.entries() {
            if let fluent_syntax::ast::Entry::Message(message) = entry {
                assert!(bundle.has_message(message.id.name), "{}", message.id.name);
            }
        }
    }

    #[test]
    fn missing_keys_show_the_key() {
        assert_eq!(tr_args("no-such-key", &[]), "no-such-key");
    }
}
