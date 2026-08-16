import { describe, expect, it } from 'vitest';

import { createBoundedQueue } from './queue';

describe('createBoundedQueue', () => {
  it('drains items in the order they were pushed', () => {
    const queue = createBoundedQueue<number>(10);
    queue.push(1);
    queue.push(2);
    queue.push(3);
    expect(queue.drain(10)).toEqual([1, 2, 3]);
    expect(queue.length).toBe(0);
  });

  it('drains at most the requested amount, leaving the rest queued', () => {
    const queue = createBoundedQueue<number>(10);
    queue.push(1);
    queue.push(2);
    queue.push(3);
    expect(queue.drain(2)).toEqual([1, 2]);
    expect(queue.drain(2)).toEqual([3]);
  });

  it('drops the oldest item and counts it once the bound is exceeded', () => {
    const queue = createBoundedQueue<number>(2);
    queue.push(1);
    queue.push(2);
    queue.push(3);
    expect(queue.length).toBe(2);
    expect(queue.dropped).toBe(1);
    expect(queue.drain(10)).toEqual([2, 3]);
  });

  it('keeps counting drops as the bound keeps being exceeded', () => {
    const queue = createBoundedQueue<number>(1);
    for (let value = 0; value < 5; value += 1) queue.push(value);
    expect(queue.length).toBe(1);
    expect(queue.dropped).toBe(4);
    expect(queue.drain(10)).toEqual([4]);
  });
});
