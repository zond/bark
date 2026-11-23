// Which language a reader gets. The English pages stand at the site's
// root and each translation in a folder named for its language, with the
// same file names. A reader who opens an English page is sent to the
// language they chose here before, or else to the first language of
// their browser's that the site has. A translated page sends nobody
// anywhere: whoever was given its link asked for it.
(() => {
const LANGS = ['en', 'sv', 'uk'], KEY = 'bark-lang';
const here = document.documentElement.lang;
// The same page in `lang`, at the place in it the reader is.
const same = lang => {
  const file = location.pathname.split('/').pop();
  return (here == 'en' ? '' : '../') + (lang == 'en' ? '' : lang + '/') + file + location.search + location.hash;
};
// A browser may keep no storage, or refuse it: then nothing is kept.
let chosen = null;
try { chosen = localStorage.getItem(KEY); } catch {}
if (here == 'en') {
  const spoken = (navigator.languages || [navigator.language || 'en']).map(l => l.toLowerCase().split('-')[0]);
  const want = LANGS.includes(chosen) ? chosen : spoken.find(l => LANGS.includes(l));
  if (want && want != 'en') location.replace(same(want));
}
addEventListener('DOMContentLoaded', () => {
  document.querySelectorAll('.lang a').forEach(a => a.addEventListener('click', e => {
    try { localStorage.setItem(KEY, a.hreflang); } catch {}
    e.preventDefault(); location.assign(same(a.hreflang));
  }));
});
})();
