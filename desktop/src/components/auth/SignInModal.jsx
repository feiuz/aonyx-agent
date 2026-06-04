import { X } from "lucide-react";
import { useAuth } from "../../context/AuthContext";

// Device-code sign-in modal: shows the user code while we poll for approval.
// The system browser is opened by the Rust side (account_device_start).
export default function SignInModal() {
  const { pending, cancelSignIn } = useAuth();
  if (!pending) return null;

  const err =
    pending.error ||
    (pending.status === "denied" ? "Accès refusé." : pending.status === "expired" ? "Code expiré." : null);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
      onClick={cancelSignIn}
    >
      <div
        className="w-[min(420px,92%)] rounded-xl border border-aonyx-300 dark:border-aonyx-700 bg-white dark:bg-aonyx-900 p-6 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-4">
          <h2 className="font-cond uppercase tracking-wide text-lg text-aonyx-900 dark:text-aonyx-100">
            Connexion aonyx-account
          </h2>
          <button onClick={cancelSignIn} className="text-aonyx-500 hover:text-aonyx-900 dark:hover:text-aonyx-100" aria-label="Fermer">
            <X className="w-5 h-5" />
          </button>
        </div>

        {err ? (
          <p className="text-sm text-red-500">{err}</p>
        ) : (
          <>
            <p className="text-sm text-aonyx-600 dark:text-aonyx-400">
              Ton navigateur s'est ouvert sur{" "}
              <span className="font-mono text-aonyx-800 dark:text-aonyx-200">{pending.verificationUrl}</span>.
              Vérifie que ce code y figure, puis approuve&nbsp;:
            </p>
            <div className="my-5 text-center">
              <div className="inline-block px-6 py-3 rounded-lg bg-aonyx-100 dark:bg-aonyx-950 border border-aonyx-200 dark:border-aonyx-800 font-mono text-2xl tracking-[0.3em] text-aonyx-900 dark:text-aonyx-100 select-text">
                {pending.userCode}
              </div>
            </div>
            <div className="flex items-center justify-center gap-2 text-xs text-aonyx-500">
              <span className="w-3.5 h-3.5 rounded-full border-2 border-aonyx-400 border-t-transparent animate-spin" />
              En attente d'approbation…
            </div>
          </>
        )}
      </div>
    </div>
  );
}
