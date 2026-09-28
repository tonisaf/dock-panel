/**
 * Instant answers for the search bar: arithmetic ("=2*(3+4)", "1200/3",
 * "15% от 2400") and currency at the Central Bank's rate ("100 usd",
 * "50 € в рублях", "1000 руб в долларах").
 */

export interface Answer {
  /** What the row shows, e.g. "= 7" or "100 USD = 9 250,00 ₽". */
  text: string;
  /** What Enter copies. */
  copy: string;
  detail: string;
}

const fmt = (n: number, digits = 10) => n.toLocaleString("ru-RU", { maximumFractionDigits: digits });
/** Plain number for the clipboard: comma decimals, no grouping. */
const plain = (n: number, digits = 10) =>
  n.toLocaleString("ru-RU", { maximumFractionDigits: digits, useGrouping: false });

// ---- arithmetic -----------------------------------------------------------------

type Tok = { t: "num"; v: number } | { t: "op"; v: string };

function tokenize(s: string): Tok[] | null {
  const out: Tok[] = [];
  let i = 0;
  while (i < s.length) {
    const c = s[i];
    if (c === " ") {
      i++;
      continue;
    }
    const num = /^\d+(?:[.,]\d+)?/.exec(s.slice(i));
    if (num) {
      out.push({ t: "num", v: Number(num[0].replace(",", ".")) });
      i += num[0].length;
      continue;
    }
    const op = { "×": "*", "·": "*", "÷": "/", "−": "-", "–": "-" }[c] ?? c;
    if ("+-*/^()%".includes(op)) {
      out.push({ t: "op", v: op });
      i++;
      continue;
    }
    return null;
  }
  return out;
}

/** Recursive descent: + - * / ^, parentheses, unary minus, postfix %. */
function evaluate(tokens: Tok[]): number | null {
  let at = 0;
  const peek = () => tokens[at];
  const isOp = (v: string) => peek()?.t === "op" && peek()!.v === v;
  const expr = (): number => {
    let v = term();
    while (isOp("+") || isOp("-")) {
      const plus = tokens[at++].v === "+";
      const start = at;
      let r = term();
      // "200 + 15%" is 15% of 200, as on a calculator.
      if (at - start === 2 && tokens[start].t === "num" && tokens[start + 1].v === "%") r = v * r;
      v = plus ? v + r : v - r;
    }
    return v;
  };
  const term = (): number => {
    let v = power();
    while (isOp("*") || isOp("/")) v = tokens[at++].v === "*" ? v * power() : v / power();
    return v;
  };
  const power = (): number => {
    const base = unary();
    if (isOp("^")) {
      at++;
      return base ** power();
    }
    return base;
  };
  const unary = (): number => {
    if (isOp("-")) {
      at++;
      return -unary();
    }
    if (isOp("+")) {
      at++;
      return unary();
    }
    let v = primary();
    while (isOp("%")) {
      at++;
      v /= 100;
    }
    return v;
  };
  const primary = (): number => {
    const tok = tokens[at++];
    if (!tok) throw new Error("end");
    if (tok.t === "num") return tok.v;
    if (tok.v === "(") {
      const v = expr();
      if (!isOp(")")) throw new Error("paren");
      at++;
      return v;
    }
    throw new Error("token");
  };
  try {
    const v = expr();
    return at === tokens.length && Number.isFinite(v) ? v : null;
  } catch {
    return null;
  }
}

function arithmetic(q: string): Answer | null {
  const s = q.trim().replace(/^=\s*/, "");
  // "15% от 2400", "15% of 2400"
  const pct = /^(\d+(?:[.,]\d+)?)\s*%\s*(?:от|of)\s*(\d+(?:[.,]\d+)?)$/i.exec(s);
  if (pct) {
    const v = (Number(pct[1].replace(",", ".")) / 100) * Number(pct[2].replace(",", "."));
    return { text: `= ${fmt(v)}`, copy: plain(v), detail: "Enter — скопировать" };
  }
  // A lone number isn't a question; an expression needs an operator (or a leading "=").
  if (!q.trim().startsWith("=") && !/\d\s*[-+*/^×÷%]/.test(s) && !/[-+*/^×÷]\s*[\d(]/.test(s)) return null;
  const tokens = tokenize(s);
  if (!tokens || !tokens.some((t) => t.t === "num")) return null;
  const v = evaluate(tokens);
  return v == null ? null : { text: `= ${fmt(v)}`, copy: plain(v), detail: "Enter — скопировать" };
}

// ---- currency ---------------------------------------------------------------------

const UNITS: [RegExp, string][] = [
  [/^(\$|usd|доллар[а-яёa-z]*|долл\.?|бакс[а-яёa-z]*)$/i, "USD"],
  [/^(€|eur|евро)$/i, "EUR"],
  [/^(₽|rub|руб[а-яёa-z]*|р\.?)$/i, "RUB"],
  [/^(¥|cny|юан[а-яёa-z]*)$/i, "CNY"],
  [/^(£|gbp|фунт[а-яёa-z]*)$/i, "GBP"],
  [/^(jpy|йен[а-яёa-z]*|иен[а-яёa-z]*)$/i, "JPY"],
  [/^(kzt|тенге|₸)$/i, "KZT"],
  [/^(try|лир[а-яёa-z]*|₺)$/i, "TRY"],
  [/^(byn|бел[а-яёa-z]*)$/i, "BYN"],
  [/^(uah|грив[а-яёa-z]*|₴)$/i, "UAH"],
  [/^(amd|драм[а-яёa-z]*)$/i, "AMD"],
  [/^(gel|лари)$/i, "GEL"],
  [/^(aed|дирхам[а-яёa-z]*)$/i, "AED"],
];

const SYMBOL: Record<string, string> = { RUB: "₽", USD: "$", EUR: "€", CNY: "¥", GBP: "£" };

function unit(word: string, known: Set<string> | null): string | null {
  const w = word.trim();
  for (const [re, code] of UNITS) if (re.test(w)) return code;
  const code = w.toUpperCase();
  return /^[A-Z]{3}$/.test(code) && (!known || known.has(code)) ? code : null;
}

export interface CurrencyQuery {
  amount: number;
  from: string;
  to: string | null;
}

/** "100 usd", "$100", "50 € в рублях", "1000 руб to usd" → parts, or null. */
export function parseCurrency(q: string): CurrencyQuery | null {
  const s = q.trim().toLowerCase();
  const m =
    /^(\d+(?:[.,]\d+)?)\s*([^\d\s]+\.?)(?:\s+(?:в|во|to|in|->|=)\s+([^\d\s]+))?$/.exec(s) ??
    /^([$€£¥₽])\s*(\d+(?:[.,]\d+)?)(?:\s+(?:в|во|to|in|->|=)\s+([^\d\s]+))?$/.exec(s);
  if (!m) return null;
  const [amountStr, fromStr] = /^\d/.test(m[1]) ? [m[1], m[2]] : [m[2], m[1]];
  const from = unit(fromStr, null);
  if (!from) return null;
  const to = m[3] ? unit(m[3], null) : null;
  if (m[3] && !to) return null;
  return { amount: Number(amountStr.replace(",", ".")), from, to };
}

export interface Rates {
  date: string;
  rub: Record<string, number>;
}

export function convert(c: CurrencyQuery, rates: Rates): Answer | null {
  const to = c.to ?? (c.from === "RUB" ? "USD" : "RUB");
  const [a, b] = [rates.rub[c.from], rates.rub[to]];
  if (!a || !b) return null;
  const v = (c.amount * a) / b;
  const sym = (code: string) => SYMBOL[code] ?? code;
  const date = rates.date ? new Date(rates.date).toLocaleDateString("ru-RU", { day: "numeric", month: "long" }) : "";
  return {
    text: `${fmt(c.amount)} ${c.from} = ${v.toLocaleString("ru-RU", { minimumFractionDigits: 2, maximumFractionDigits: 2 })} ${sym(to)}`,
    copy: plain(v, 2),
    detail: `Курс ЦБ${date ? ` на ${date}` : ""} · Enter — скопировать`,
  };
}

export function answer(q: string): Answer | null {
  return arithmetic(q);
}
