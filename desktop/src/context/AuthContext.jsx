import { createContext, useContext, useState } from "react";

// Offline-first (ADR-011): the local agent works without an account. Signing in
// unlocks cloud/licence/sync. The real device-code flow (account_* Rust commands
// + keyring) lands in Phase P4 — for now this is a non-blocking stub so the shell
// and the user widget render.

const AuthContext = createContext(null);

export function AuthProvider({ children }) {
  const [user, setUser] = useState(null);
  const [loading] = useState(false);

  const isAuthenticated = !!user;

  // TODO P4: account_device_start/poll via aonyx-account device-code grant.
  const signIn = () => {
    console.info("[auth] device-code sign-in lands in P4");
  };
  const logout = () => setUser(null);

  return (
    <AuthContext.Provider value={{ user, isAuthenticated, loading, signIn, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export const useAuth = () => useContext(AuthContext);
