import { safeInvoke } from "../config/bridge";

const DEFAULT = {
  telegram: { allowed: [], hasToken: false },
  discord: { allowed: [], hasToken: false },
};

/** Read messaging-channel config (allowed ids + whether a token is stored). */
export const readMessaging = () => safeInvoke("read_messaging", undefined, DEFAULT);

/** Save a channel's allowed ids (config.toml) and token (keyring, when given). */
export const saveMessaging = (channel, allowed, token) =>
  safeInvoke("save_messaging", { channel, allowed, token: token || null });
