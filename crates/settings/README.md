# Language dictionaries

UI code uses `TextKey` rather than English strings as lookup keys. English is
stored once in a lazily initialized hashmap. A `LanguagePack` owns a thread-safe,
dynamically dispatched `Dictionary`, so different dictionary implementations can
be selected at runtime.

```rust
use settings::{Settings, langs::{HashMapDictionary, TextKey}};

let custom = HashMapDictionary::new([
    (TextKey::TuiName, "Gestion des stocks".to_owned()),
]);
let pack = custom.into_language_pack();
assert_eq!(pack.get(TextKey::TuiName), Some("Gestion des stocks"));
let settings = Settings::default();
settings.swap_lang(pack);
assert_eq!(settings.lang().get(TextKey::TuiName), Some("Gestion des stocks"));
```

`LanguagePack<English>` labels an English pack; `LanguagePack<CustomLanguage>`
labels a custom pack. These markers describe intent and do not validate the
dictionary contents. Both markers and the `Language` trait are public.
`HashMapDictionary::into_language_pack()` constructs a custom pack.

`LanguagePack` defaults its type parameter to `RuntimeLanguage`.
`LanguagePack::default()` provides English in a runtime pack. Use
`typed_pack.into_runtime()` to erase a marker when initializing
`Settings::new(Some(pack), database_settings)`. `Settings::new(None, ...)` also
selects English. The previous `LanguageType` / `load_language` API is replaced
by typed pack construction.

`pack.get(key)` tries the selected dictionary, then English. It returns `None`
if neither contains the key. Empty strings are valid translations. Use
`pack.dict()` for raw lookups or iteration without fallback. Iteration borrows
from the dictionary and has no defined order. Runtime dictionaries own their
strings, and duplicate keys keep the last value.

When adding UI text, add a `TextKey` variant, include it in `TextKey::ALL`, and
provide its English string in `english.rs`. The coverage test checks that all
listed keys have nonempty English text. Keep text keys independent of translated
wording. For future interpolation or pluralization, add a formatting API when
the UI needs it; dictionary values currently represent plain text only.

`Settings` stores a runtime pack in `ArcSwap`, allowing `swap_lang` to accept
any language marker. `settings.lang()` returns a snapshot: keep it for a complete
render so all text uses the same language. Existing snapshots remain valid after
a swap. Share `Settings` through `Arc<Settings>` when multiple threads need it.

`settings.database_settings()` exposes connection configuration only. Defaults
are `sqlite://database/sqlite.db` and 25 maximum connections. Pool creation,
database file creation, migrations, and queries belong to persistence; constructing
settings performs no database I/O.
