import packageMetadata from '../../package.json' with { type: 'json' };

const appVersion = packageMetadata.version;

export default appVersion;
