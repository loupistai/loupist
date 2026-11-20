use std::sync::LazyLock;

use serde_json::Value;

pub(crate) fn resolve_language(language: &mut String) {
    if language == "system" {
        *language = crate::briefings::system_language();
    }
}

#[derive(Clone, Copy)]
pub(crate) enum InterfaceLocale {
    English,
    Korean,
}

impl InterfaceLocale {
    pub(crate) fn from_language(language: &str) -> Self {
        if language == "ko" {
            Self::Korean
        } else {
            Self::English
        }
    }

    pub(crate) fn catalog(self) -> &'static InterfaceCatalog {
        match self {
            Self::English => &ENGLISH,
            Self::Korean => &KOREAN,
        }
    }
}

pub(crate) struct InterfaceCatalog {
    pub names: Value,
    pub research_sessions_heading: String,
    pub market_data_group: String,
}

impl InterfaceCatalog {
    fn new(agent: &str, app: &str, connections: &str) -> Self {
        fn label(catalog: &str, path: &str) -> String {
            let catalog: Value = serde_json::from_str(catalog).expect("bundled UI locale");
            catalog
                .pointer(path)
                .and_then(Value::as_str)
                .expect("bundled UI label")
                .to_owned()
        }

        let agent: Value = serde_json::from_str(agent).expect("bundled agent locale");
        Self {
            names: agent["names"].clone(),
            research_sessions_heading: label(app, "/shell/sessions"),
            market_data_group: label(connections, "/catalog/MarketData"),
        }
    }
}

static ENGLISH: LazyLock<InterfaceCatalog> = LazyLock::new(|| {
    InterfaceCatalog::new(
        include_str!("../../../src/i18n/locales/en/agent.json"),
        include_str!("../../../src/i18n/locales/en/app.json"),
        include_str!("../../../src/i18n/locales/en/connections.json"),
    )
});
static KOREAN: LazyLock<InterfaceCatalog> = LazyLock::new(|| {
    InterfaceCatalog::new(
        include_str!("../../../src/i18n/locales/ko/agent.json"),
        include_str!("../../../src/i18n/locales/ko/app.json"),
        include_str!("../../../src/i18n/locales/ko/connections.json"),
    )
});
