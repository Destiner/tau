import { readFileSync } from 'node:fs';

function requirePolicy(value: unknown): asserts value {
  if (!value) throw new Error('Native package policy rejected');
}
export function validateInputs(
  ref: string,
  sha: string,
  commit: string,
  version: string,
  rootVersion: string,
  cargo: string,
  lock: string,
): void {
  requirePolicy(
    ref === 'refs/heads/main' &&
      /^[a-f0-9]{40}$/.test(commit) &&
      sha === commit,
  );
  requirePolicy(
    /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version) &&
      rootVersion === version,
  );
  requirePolicy(/^version = "([^"]+)"$/m.exec(cargo)?.[1] === version);
  requirePolicy(
    new RegExp(
      `\\[\\[package\\]\\]\\nname = "tau-gpui"\\nversion = "${version.replaceAll('.', '\\.')}"(?:\\n|$)`,
    ).test(lock),
  );
}
export type Api = (path: string, allowMissing?: boolean) => Promise<unknown>;
type Run = {
  id: number;
  workflow_id: number;
  path: string;
  event: string;
  head_branch: string;
  head_sha: string;
  status: string;
  conclusion: string;
};
export async function verifyGitHub(
  api: Api,
  commit: string,
  version: string,
): Promise<number> {
  const workflow = (await api('/actions/workflows/ci.yml')) as {
    id: number;
    path: string;
  };
  requirePolicy(
    Number.isSafeInteger(workflow.id) &&
      workflow.path === '.github/workflows/ci.yml',
  );
  const response = (await api(
    `/actions/workflows/ci.yml/runs?event=push&branch=main&head_sha=${commit}&per_page=100`,
  )) as { workflow_runs: Run[] };
  requirePolicy(Array.isArray(response.workflow_runs));
  // The latest matching CI run must be completed successfully; never accept another workflow's check.
  const runs = response.workflow_runs
    .filter(
      (run) =>
        run.workflow_id === workflow.id &&
        run.path === workflow.path &&
        run.event === 'push' &&
        run.head_branch === 'main' &&
        run.head_sha === commit,
    )
    .sort((a, b) => b.id - a.id);
  const run = runs[0];
  requirePolicy(
    run && run.status === 'completed' && run.conclusion === 'success',
  );
  const jobs = (await api(
    `/actions/runs/${run.id}/jobs?filter=latest&per_page=100`,
  )) as {
    total_count: number;
    jobs: {
      name: string;
      status: string;
      conclusion: string;
      head_sha: string;
    }[];
  };
  requirePolicy(
    jobs.total_count === jobs.jobs.length &&
      jobs.jobs.filter(
        (job) =>
          job.name === 'check' &&
          job.head_sha === commit &&
          job.status === 'completed' &&
          job.conclusion === 'success',
      ).length === 1,
  );
  const tag = `v${version}`;
  const matching = (await api(`/git/matching-refs/tags/${tag}`)) as {
    ref: string;
  }[];
  requirePolicy(
    Array.isArray(matching) &&
      !matching.some((ref) => ref.ref === `refs/tags/${tag}`),
  );
  requirePolicy((await api(`/git/ref/tags/${tag}`, true)) === null);
  return run.id;
}
if (import.meta.main) {
  try {
    const {
      GITHUB_REF,
      GITHUB_SHA,
      COMMIT,
      VERSION,
      GITHUB_REPOSITORY,
      GH_TOKEN,
    } = process.env;
    requirePolicy(
      GITHUB_REF &&
        GITHUB_SHA &&
        COMMIT &&
        VERSION &&
        GITHUB_REPOSITORY &&
        GH_TOKEN,
    );
    validateInputs(
      GITHUB_REF,
      GITHUB_SHA,
      COMMIT,
      VERSION,
      JSON.parse(readFileSync('package.json', 'utf8')).version,
      readFileSync('gpui/Cargo.toml', 'utf8'),
      readFileSync('gpui/Cargo.lock', 'utf8'),
    );
    const id = await verifyGitHub(
      async (path, allowMissing) => {
        const response = await fetch(
          `https://api.github.com/repos/${GITHUB_REPOSITORY}${path}`,
          {
            headers: {
              Authorization: `Bearer ${GH_TOKEN}`,
              Accept: 'application/vnd.github+json',
              'X-GitHub-Api-Version': '2022-11-28',
            },
            redirect: 'error',
            signal: AbortSignal.timeout(30_000),
          },
        );
        if (allowMissing && response.status === 404) return null;
        requirePolicy(response.ok);
        return response.json();
      },
      COMMIT,
      VERSION,
    );
    console.log(
      `Exact-source CI push run accepted: ${id}; public v${VERSION} tag absent (not a draft-release query).`,
    );
  } catch {
    console.error('Native package preflight failed closed');
    process.exitCode = 1;
  }
}
