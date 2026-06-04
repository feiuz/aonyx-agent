import { invoke, safeInvoke } from "../config/bridge";

// aonyx-account base URL (prod default; overridable for staging/dev).
const base = () => localStorage.getItem("aonyx.accountUrl") || "https://account.aonyx.fr";
export const getAccountUrl = base;
export const setAccountUrl = (u) => localStorage.setItem("aonyx.accountUrl", u);

// Device-code grant (ADR-011): start → poll → store. Tokens live in the OS keyring.
export const deviceStart = () => invoke("account_device_start", { base: base() });
export const devicePoll = (deviceCode) => invoke("account_device_poll", { base: base(), deviceCode });
export const store = (access, refresh) => invoke("account_store", { access, refresh });
export const hasToken = () => safeInvoke("account_has_token", undefined, false);
export const me = () => invoke("account_me", { base: base() });
export const logout = () => safeInvoke("account_logout");
