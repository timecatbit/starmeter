// Types for phone-api.mjs, for the dev server's config and the tests.
export declare const PHONE_KEY: string;
export declare function phoneAnswer(data: unknown, request: {
  pathname: string; params: URLSearchParams; given: string | undefined; key?: string; logo: (file: string) => string; swiftbar?: boolean;
  live?: ((data: unknown) => unknown) | null;
}): { status: number; type: string; body: string };
export declare function still(data: unknown): unknown;
export declare function liveWork(options?: { now?: () => number; length?: number; claudeEnds?: number; codexEnds?: number }): (data: unknown) => unknown;
export declare function phoneFixtures(): import('vite').Plugin;
