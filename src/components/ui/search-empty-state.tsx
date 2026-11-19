import illustration from "@/assets/illustrations/agent-search-empty.png";
import "./search-empty-state.css";

interface SearchEmptyStateProps {
  title: string;
  hint: string;
}

export function SearchEmptyState({ title, hint }: SearchEmptyStateProps) {
  return (
    <div className="search-empty-state">
      <img src={illustration} alt="" width={340} height={260} />
      <div role="status">
        <h2>{title}</h2>
        <p>{hint}</p>
      </div>
    </div>
  );
}
