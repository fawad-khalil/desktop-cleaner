/** A name/path search: plain text or a regular expression. */
export interface SearchFilter {
  pattern: string;
  regex: boolean;
  case_sensitive: boolean;
}

export const emptySearch = (): SearchFilter => ({ pattern: "", regex: false, case_sensitive: false });

export interface Matcher {
  /** Null when the pattern is empty (everything matches). */
  test: ((s: string) => boolean) | null;
  error: string;
}

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Compiles a filter for in-page lists. Invalid patterns match nothing and report why. */
export function matcher(f: SearchFilter): Matcher {
  if (!f.pattern) return { test: null, error: "" };
  try {
    const re = new RegExp(f.regex ? f.pattern : escape(f.pattern), f.case_sensitive ? "" : "i");
    return { test: (s) => re.test(s), error: "" };
  } catch (e) {
    return { test: () => false, error: e instanceof Error ? e.message.replace(/^Invalid regular expression: /, "") : String(e) };
  }
}

/** The filter to send to the backend, or null when there's nothing to search for. */
export const forBackend = (f: SearchFilter) => (f.pattern ? { ...f } : null);
