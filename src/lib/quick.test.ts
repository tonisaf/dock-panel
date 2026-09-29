import { describe, expect, it } from "vitest";
import { answer, convert, parseCurrency } from "./quick";

const copy = (q: string) => answer(q)?.copy ?? null;

describe("arithmetic", () => {
  it("evaluates with precedence, parentheses and powers", () => {
    expect(copy("=2*(3+4)")).toBe("14");
    expect(copy("2+3*4")).toBe("14");
    expect(copy("2^3^2")).toBe("512");
    expect(copy("-(2+3)")).toBe("-5");
  });

  it("takes comma decimals and typographic operators", () => {
    expect(copy("1,5 × 4")).toBe("6");
    expect(copy("10 ÷ 4")).toBe("2,5");
    expect(copy("10 − 3")).toBe("7");
  });

  it("treats % as a calculator does", () => {
    expect(copy("200 + 15%")).toBe("230");
    expect(copy("200 - 10%")).toBe("180");
    expect(copy("15% от 2400")).toBe("360");
    expect(copy("15% of 2400")).toBe("360");
  });

  it("ignores a lone number and plain words", () => {
    expect(answer("42")).toBeNull();
    expect(answer("chrome")).toBeNull();
    expect(answer("")).toBeNull();
  });

  it("rejects broken and non-finite input", () => {
    expect(answer("=2+")).toBeNull();
    expect(answer("=(2+3")).toBeNull();
    expect(answer("=1/0")).toBeNull();
    expect(answer("=2 $ 3")).toBeNull();
  });
});

describe("parseCurrency", () => {
  it("reads amount and unit in several spellings", () => {
    expect(parseCurrency("100 usd")).toEqual({ amount: 100, from: "USD", to: null });
    expect(parseCurrency("$100")).toEqual({ amount: 100, from: "USD", to: null });
    expect(parseCurrency("2,5 евро")).toEqual({ amount: 2.5, from: "EUR", to: null });
  });

  it("reads a target currency", () => {
    expect(parseCurrency("50 € в рублях")).toEqual({ amount: 50, from: "EUR", to: "RUB" });
    expect(parseCurrency("1000 руб to usd")).toEqual({ amount: 1000, from: "RUB", to: "USD" });
  });

  it("rejects unknown units and a target it cannot read", () => {
    expect(parseCurrency("100 кг")).toBeNull();
    expect(parseCurrency("100 usd в кг")).toBeNull();
    expect(parseCurrency("usd")).toBeNull();
  });
});

describe("convert", () => {
  const rates = { date: "", rub: { RUB: 1, USD: 90, EUR: 100 } };

  it("converts through rubles", () => {
    expect(convert({ amount: 2, from: "USD", to: "RUB" }, rates)?.copy).toBe("180");
    expect(convert({ amount: 100, from: "USD", to: "EUR" }, rates)?.copy).toBe("90");
  });

  it("defaults to rubles, and from rubles to dollars", () => {
    expect(convert({ amount: 1, from: "EUR", to: null }, rates)?.copy).toBe("100");
    expect(convert({ amount: 180, from: "RUB", to: null }, rates)?.copy).toBe("2");
  });

  it("gives nothing for a currency without a rate", () => {
    expect(convert({ amount: 1, from: "KZT", to: null }, rates)).toBeNull();
  });
});
