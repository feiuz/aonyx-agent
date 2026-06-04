import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import * as account from "../services/accountService";
import { isTauri } from "../config/bridge";

// Offline-first (ADR-011): the local agent works without an account. Signing in
// (device-code grant via aonyx-account) unlocks cloud/licence/sync. Tokens are
// stored in the OS keyring by the Rust side.

const AuthContext = createContext(null);

const normalize = (u) =>
  u
    ? {
        email: u.email,
        tier: u.profile?.subscription || u.subscription || "FREE",
        name: [u.profile?.firstName, u.profile?.lastName].filter(Boolean).join(" "),
      }
    : null;

export function AuthProvider({ children }) {
  const [user, setUser] = useState(null);
  const [pending, setPending] = useState(null); // { userCode, verificationUrl, status, error }
  const pollRef = useRef(null);

  const isAuthenticated = !!user;
  // Licence gating primitive (ADR-013). FREE by default; PREMIUM/ENTERPRISE
  // unlock cloud features once they exist (OQ4-bis defines the feature split).
  const isPremium = user?.tier === "PREMIUM" || user?.tier === "ENTERPRISE";

  // Restore session on mount (token in keyring → fetch profile).
  useEffect(() => {
    if (!isTauri()) return;
    (async () => {
      if (await account.hasToken()) {
        try {
          const me = await account.me();
          setUser(normalize(me?.user || me));
        } catch {
          /* token invalid or backend unreachable — stay logged out, offline-first */
        }
      }
    })();
    return () => {
      if (pollRef.current) clearInterval(pollRef.current);
    };
  }, []);

  const cancelSignIn = useCallback(() => {
    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
    setPending(null);
  }, []);

  const signIn = useCallback(async () => {
    if (!isTauri()) {
      setPending({ error: "Connexion disponible uniquement dans l'app (Tauri)." });
      return;
    }
    try {
      const r = await account.deviceStart();
      const deviceCode = r.deviceCode;
      const interval = (r.interval || 5) * 1000;
      const deadline = Date.now() + (r.expiresIn || 600) * 1000;
      setPending({ userCode: r.userCode, verificationUrl: r.verificationUrl, status: "pending" });

      if (pollRef.current) clearInterval(pollRef.current);
      pollRef.current = setInterval(async () => {
        if (Date.now() > deadline) {
          clearInterval(pollRef.current);
          pollRef.current = null;
          setPending((p) => ({ ...p, status: "expired", error: "Code expiré." }));
          return;
        }
        try {
          const res = await account.devicePoll(deviceCode);
          if (res?.status === "approved" && res.tokens) {
            clearInterval(pollRef.current);
            pollRef.current = null;
            await account.store(res.tokens.accessToken, res.tokens.refreshToken);
            setUser(normalize(res.user));
            setPending(null);
          } else if (res?.status === "denied" || res?.status === "expired") {
            clearInterval(pollRef.current);
            pollRef.current = null;
            setPending((p) => ({ ...p, status: res.status, error: res.error || "Refusé ou expiré." }));
          }
        } catch {
          /* transient network error — keep polling until the deadline */
        }
      }, interval);
    } catch (e) {
      setPending({ error: String(e) });
    }
  }, []);

  const logout = useCallback(async () => {
    await account.logout();
    setUser(null);
  }, []);

  return (
    <AuthContext.Provider value={{ user, isAuthenticated, isPremium, pending, signIn, cancelSignIn, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export const useAuth = () => useContext(AuthContext);
