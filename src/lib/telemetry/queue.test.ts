import { describe, expect, it } from 'vitest';

import { createBoundedQueue } from './queue';

describe('createBoundedQueue', () => {
  it('drains in FIFO order up to the requested amount', () => {
    const queue = createBoundedQueue<number>(10);
    queue.push(1);
    queue.push(2);
    queue.push(3);
    expect(queue.drain(2)).toEqual([1, 2]);
    expect(queue.drain(2)).toEqual([3]);
    expect(queue.length).toBe(0);
  });

  it('evicts the oldest entries and counts every overflow', () => {
    const queue = createBoundedQueue<number>(2);
    queue.push(1);
    queue.push(2);
    queue.push(3);
    expect(queue.length).toBe(2);
    expect(queue.dropped).toBe(1);
    expect(queue.drain(10)).toEqual([2, 3]);
    const single = createBoundedQueue<number>(1);
    for (let value = 0; value < 5; value += 1) single.push(value);
    expect(single.length).toBe(1);
    expect(single.dropped).toBe(4);
    expect(single.drain(10)).toEqual([4]);
  });
});
