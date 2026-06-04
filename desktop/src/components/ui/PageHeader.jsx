export default function PageHeader({ icon: Icon, title, subtitle, actions }) {
  return (
    <header className="flex items-center justify-between h-14 px-5 flex-shrink-0 border-b border-aonyx-200 dark:border-aonyx-800">
      <div className="flex items-center gap-2.5 min-w-0">
        {Icon && <Icon className="w-5 h-5 text-aonyx-500 flex-shrink-0" strokeWidth={1.75} />}
        <div className="min-w-0">
          <h1 className="font-cond uppercase tracking-wide text-lg leading-none truncate text-aonyx-900 dark:text-aonyx-100">
            {title}
          </h1>
          {subtitle && <p className="mt-0.5 text-xs text-aonyx-500 truncate">{subtitle}</p>}
        </div>
      </div>
      {actions}
    </header>
  );
}
