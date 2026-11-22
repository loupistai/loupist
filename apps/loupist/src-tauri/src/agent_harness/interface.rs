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
    fn new(agent: &str, app: &str, connections: &str, knowledge: &str) -> Self {
        fn label(catalog: &Value, path: &str) -> String {
            catalog
                .pointer(path)
                .and_then(Value::as_str)
                .expect("bundled UI label")
                .to_owned()
        }

        let agent: Value = serde_json::from_str(agent).expect("bundled agent locale");
        let app: Value = serde_json::from_str(app).expect("bundled app locale");
        let connections: Value =
            serde_json::from_str(connections).expect("bundled connections locale");
        let knowledge: Value = serde_json::from_str(knowledge).expect("bundled knowledge locale");
        let mut names = agent["names"]
            .as_object()
            .expect("bundled product names")
            .clone();
        let world = names.remove("map").expect("bundled World name");
        names.insert("world".into(), world);
        for (key, catalog, path) in [
            ("lens", &app, "/shell/tabs/lens"),
            ("following", &app, "/shell/tabs/following"),
            ("journal", &knowledge, "/map/libraryJournal"),
            ("sources_library", &knowledge, "/map/sources"),
            ("perspective", &knowledge, "/memoryPage/groups/perspective"),
            (
                "your_principles",
                &knowledge,
                "/memoryPage/groups/principle",
            ),
            ("methods", &knowledge, "/memoryPage/groups/method"),
            ("topics", &knowledge, "/memoryPage/groups/subject"),
            ("ideas", &knowledge, "/memoryPage/groups/idea"),
        ] {
            names.insert(key.into(), Value::String(label(catalog, path)));
        }
        Self {
            names: Value::Object(names),
            research_sessions_heading: label(&app, "/shell/sessions"),
            market_data_group: label(&connections, "/catalog/MarketData"),
        }
    }
}

static ENGLISH: LazyLock<InterfaceCatalog> = LazyLock::new(|| {
    InterfaceCatalog::new(
        include_str!("../../../src/i18n/locales/en/agent.json"),
        include_str!("../../../src/i18n/locales/en/app.json"),
        include_str!("../../../src/i18n/locales/en/connections.json"),
        include_str!("../../../src/i18n/locales/en/knowledge.json"),
    )
});
static KOREAN: LazyLock<InterfaceCatalog> = LazyLock::new(|| {
    InterfaceCatalog::new(
        include_str!("../../../src/i18n/locales/ko/agent.json"),
        include_str!("../../../src/i18n/locales/ko/app.json"),
        include_str!("../../../src/i18n/locales/ko/connections.json"),
        include_str!("../../../src/i18n/locales/ko/knowledge.json"),
    )
});
