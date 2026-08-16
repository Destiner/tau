/*
 * A bounded FIFO queue used as both the pre-initialization buffer (records
 * created before anything has flushed yet) and the batching buffer (records
 * waiting for the next flush): nothing distinguishes those two situations,
 * so one bounded queue serves both. Bounded so a slow or failing flush
 * cannot grow memory without limit; the oldest record is dropped first,
 * since a gap further back is preferable to losing what just happened.
 */
interface BoundedQueue<T> {
  push(item: T): void;
  drain(max: number): T[];
  readonly length: number;
  readonly dropped: number;
}

function createBoundedQueue<T>(capacity: number): BoundedQueue<T> {
  const items: T[] = [];
  let dropped = 0;

  return {
    push(item: T): void {
      if (items.length >= capacity) {
        items.shift();
        dropped += 1;
      }
      items.push(item);
    },
    drain(max: number): T[] {
      return items.splice(0, max);
    },
    get length(): number {
      return items.length;
    },
    get dropped(): number {
      return dropped;
    },
  };
}

export type { BoundedQueue };

export { createBoundedQueue };
