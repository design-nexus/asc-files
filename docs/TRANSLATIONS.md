# Translations

Files is written in English. The app's text (QML and C++) goes through Qt's translation system; the
engine's messages through `rust_i18n` (`crates/asc-files-engine/locales/en.json`).

## English plural forms

`app/i18n/asc-files_en.ts` holds only the plural forms (`%n items` → "1 item", "2 items"). After adding
or changing a string with `%n`, refresh it and fill the new forms:

```sh
cmake --build ~/.cache/asc-files-app-build --target update_translations   # in scripts/app.sh's shell
```

`tst_translations` checks the built forms.

## Another language

Add `app/i18n/asc-files_<lang>.ts` to `TS_FILES` in `qt_add_translations` (app/CMakeLists.txt), run
`update_translations`, and translate it with Qt Linguist. At start the app loads the file matching
the user's locale, falling back to English. Legacy's translations do not carry over: the strings are new.
