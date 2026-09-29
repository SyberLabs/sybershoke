// Stand-in for @upstash/redis/cloudflare: an in-memory store the runner can observe.
export class Redis {
  get(key) { return globalThis.__shoke.redis.get(key); }
  set(key, value, options) { return globalThis.__shoke.redis.set(key, value, options); }
  incr(key) { return globalThis.__shoke.redis.incr(key); }
  expire(key, seconds) { return globalThis.__shoke.redis.expire(key, seconds); }
}
