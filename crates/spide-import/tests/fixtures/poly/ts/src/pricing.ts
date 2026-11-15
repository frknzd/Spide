export interface Priced {
  price(): number;
}

export function tax(cents: number): number {
  if (cents === 0) return 0;
  return cents < 1000 ? cents / 10 : cents / 5;
}

export const parseCents = (s: string): number => {
  const v: any = JSON.parse(s);
  return Number(v);
};
