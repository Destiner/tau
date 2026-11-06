type ReloadContext = {
  isIdle?: () => boolean;
  reload?: () => Promise<void>;
};

type ReloadAPI = {
  registerCommand: (
    name: string,
    command: {
      description: string;
      handler: (args: string, ctx: ReloadContext) => Promise<void>;
    },
  ) => void;
};

export default function (pi: ReloadAPI): void {
  pi.registerCommand('tau-internal-reload-v1', {
    description: 'Private Tau resource reload bridge',
    handler: async (args, ctx) => {
      if (args.trim()) {
        throw new Error('Tau reload does not accept arguments.');
      }
      if (
        typeof ctx.isIdle !== 'function' ||
        typeof ctx.reload !== 'function'
      ) {
        throw new Error(
          'This Pi version does not support Tau resource reload.',
        );
      }
      if (!ctx.isIdle()) {
        throw new Error('Pi is busy. Wait for the current work to finish.');
      }
      // Reload invalidates this runtime. Never access its context afterward.
      try {
        await ctx.reload();
      } catch {
        throw new Error(
          'Pi resource reload failed. Review extension errors and try again.',
        );
      }
    },
  });
}
