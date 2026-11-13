// The page's words go through `t`, `tc` and `tf`: English in the scripts, and what a language says for each in the table
// `useWords` is given, by a page that has the interface's catalogs (`translations/ui`). With none, and for a word a language
// lacks, the English shows.
let words = new Map();

export const useWords = table => { words = new Map(Object.entries(table || {})); };
export const t = english => words.get(english) ?? english;
// For a word that means two things in English: `tc('status', 'Open')` is not the button's Open.
export const tc = (context, english) => words.get(`${context}\u0004${english}`) ?? english;
export function tf(english, ...values) {
  let n = 0;
  return t(english).replace(/\{\}/g, () => values[n++] ?? '');
}

