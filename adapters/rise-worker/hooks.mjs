// Module resolve hook: the Worker's two service clients become in-process stand-ins.
const STUBS = {
  '@neondatabase/serverless': new URL('./stubs/neon.mjs', import.meta.url).href,
  '@upstash/redis/cloudflare': new URL('./stubs/redis.mjs', import.meta.url).href
};

export async function resolve(specifier, context, next) {
  if (Object.hasOwn(STUBS, specifier)) return { url: STUBS[specifier], shortCircuit: true };
  return next(specifier, context);
}
