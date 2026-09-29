// Stand-in for @neondatabase/serverless: the runner answers each query from fixture rows.
export function neon() {
  return (strings, ...values) => globalThis.__shoke.neon(strings.join('?'), values);
}
