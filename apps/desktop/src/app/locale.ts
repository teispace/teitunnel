/**
 * WebKitGTK reports the POSIX locale as `navigator.language` when `LANG` is `C` or
 * `POSIX` (the default on many servers and CI runners), and that isn't a BCP 47 tag.
 * Libraries that hand it to `Intl` then throw while loading (uPlot builds its number
 * format at import time), taking the whole view with them. Replace an invalid language
 * with the locale `Intl` itself resolved, before anything reads it.
 */
export function repairNavigatorLanguage(nav: Navigator = navigator) {
  const valid = (tag: string) => {
    try {
      return Intl.getCanonicalLocales(tag).length > 0;
    } catch {
      return false;
    }
  };
  if (valid(nav.language)) return;
  const fallback = new Intl.NumberFormat().resolvedOptions().locale;
  const languages = nav.languages.filter(valid);
  Object.defineProperties(nav, {
    language: { value: fallback, configurable: true },
    languages: {
      value: Object.freeze(languages.length > 0 ? languages : [fallback]),
      configurable: true,
    },
  });
}
