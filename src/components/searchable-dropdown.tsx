import { useState, useRef, useEffect, useMemo, type ReactNode } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { ChevronDown, Search, Star } from "lucide-react";

interface SearchableDropdownProps {
  items: string[];
  selected: string | undefined;
  onSelect: (item: string) => void;
  placeholder?: string;
  label: string;
  allOption?: { label: string; value: string };
  storageKey: string; // For localStorage favorites
  onOpen?: () => void;
}

// Helper function to highlight matching text
function highlightMatch(text: string, query: string): ReactNode {
  if (!query) return text;
  const lowerText = text.toLowerCase();
  const lowerQuery = query.toLowerCase();
  const index = lowerText.indexOf(lowerQuery);
  
  if (index === -1) return text;
  
  const before = text.substring(0, index);
  const match = text.substring(index, index + query.length);
  const after = text.substring(index + query.length);
  
  return (
    <>
      {before}
      <span className="bg-accent/30 text-accent font-semibold">{match}</span>
      {after}
    </>
  );
}

export function SearchableDropdown({
  items,
  selected,
  onSelect,
  placeholder = "Select...",
  label,
  allOption,
  storageKey,
  onOpen,
}: SearchableDropdownProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [favorites, setFavorites] = useState<Set<string>>(new Set());
  const [highlightedIndex, setHighlightedIndex] = useState(0);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);

  // Load favorites from localStorage on mount
  useEffect(() => {
    try {
      const stored = localStorage.getItem(storageKey);
      if (stored) {
        setFavorites(new Set(JSON.parse(stored)));
      }
    } catch (err) {
      console.error("Failed to load favorites", err);
    }
  }, [storageKey]);

  // Save favorites to localStorage when they change
  useEffect(() => {
    try {
      localStorage.setItem(storageKey, JSON.stringify(Array.from(favorites)));
    } catch (err) {
      console.error("Failed to save favorites", err);
    }
  }, [favorites, storageKey]);

  // Reset highlighted index and refs when search query or items change
  useEffect(() => {
    setHighlightedIndex(0);
    // Reset refs array when items change
    itemRefs.current = [];
  }, [searchQuery, items]);

  const toggleFavorite = (item: string, e: React.MouseEvent) => {
    e.stopPropagation();
    setFavorites((prev) => {
      const next = new Set(prev);
      if (next.has(item)) {
        next.delete(item);
      } else {
        next.add(item);
      }
      return next;
    });
  };

  // Filter items based on search query
  const filteredItems = useMemo(() => {
    const allItems = allOption ? [allOption.value, ...items] : items;
    if (!searchQuery) return allItems;
    const lower = searchQuery.toLowerCase();
    return allItems.filter((item) => item.toLowerCase().includes(lower));
  }, [items, searchQuery, allOption]);

  // Sort items: favorites first, then by match quality (starts with query first), then alphabetically
  const sortedItems = useMemo(() => {
    if (!searchQuery) {
      return [...filteredItems].sort((a, b) => {
        const aIsFavorite = favorites.has(a);
        const bIsFavorite = favorites.has(b);
        if (aIsFavorite && !bIsFavorite) return -1;
        if (!aIsFavorite && bIsFavorite) return 1;
        return a.localeCompare(b);
      });
    }
    
    const lower = searchQuery.toLowerCase();
    return [...filteredItems].sort((a, b) => {
      const aIsFavorite = favorites.has(a);
      const bIsFavorite = favorites.has(b);
      if (aIsFavorite && !bIsFavorite) return -1;
      if (!aIsFavorite && bIsFavorite) return 1;
      
      // Prioritize items that start with the query
      const aStartsWith = a.toLowerCase().startsWith(lower);
      const bStartsWith = b.toLowerCase().startsWith(lower);
      if (aStartsWith && !bStartsWith) return -1;
      if (!aStartsWith && bStartsWith) return 1;
      
      return a.localeCompare(b);
    });
  }, [filteredItems, favorites, searchQuery]);

  // Close dropdown when clicking outside
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setIsOpen(false);
        setSearchQuery("");
        setHighlightedIndex(0);
      }
    };

    if (isOpen) {
      document.addEventListener("mousedown", handleClickOutside);
      // Focus search input when dropdown opens
      setTimeout(() => {
        searchInputRef.current?.focus();
      }, 0);
    }

    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [isOpen]);

  // Handle keyboard navigation
  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      // Only handle if the dropdown is open and we're not typing in an input
      if (e.target instanceof HTMLInputElement) {
        // Allow Tab for autocomplete
        if (e.key !== "Tab") return;
      }

      // Get current sortedItems length safely
      const itemsLength = sortedItems.length;
      if (itemsLength === 0) return;

      if (e.key === "ArrowDown") {
        e.preventDefault();
        setHighlightedIndex((prev) => {
          const maxIndex = Math.max(0, itemsLength - 1);
          const next = Math.min(prev + 1, maxIndex);
          // Scroll into view
          setTimeout(() => {
            itemRefs.current[next]?.scrollIntoView({ block: "nearest", behavior: "smooth" });
          }, 0);
          return next;
        });
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setHighlightedIndex((prev) => {
          const next = Math.max(prev - 1, 0);
          // Scroll into view
          setTimeout(() => {
            itemRefs.current[next]?.scrollIntoView({ block: "nearest", behavior: "smooth" });
          }, 0);
          return next;
        });
      } else if (e.key === "Enter") {
        e.preventDefault();
        const currentIndex = highlightedIndex;
        const safeIndex = Math.max(0, Math.min(currentIndex, itemsLength - 1));
        if (itemsLength > 0 && safeIndex >= 0 && safeIndex < itemsLength) {
          const item = sortedItems[safeIndex];
          onSelect(item);
          setIsOpen(false);
          setSearchQuery("");
          setHighlightedIndex(0);
        }
      } else if (e.key === "Escape") {
        e.preventDefault();
        setIsOpen(false);
        setSearchQuery("");
        setHighlightedIndex(0);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, sortedItems, highlightedIndex, onSelect]);

  // Get the first matching item for autocomplete suggestion
  const autocompleteSuggestion = useMemo(() => {
    if (!searchQuery || sortedItems.length === 0) return null;
    const firstMatch = sortedItems[0];
    const lowerQuery = searchQuery.toLowerCase();
    const lowerMatch = firstMatch.toLowerCase();
    if (lowerMatch.startsWith(lowerQuery) && lowerMatch !== lowerQuery) {
      return firstMatch;
    }
    return null;
  }, [searchQuery, sortedItems]);

  const displayValue = selected || placeholder;
  const isSelectedFavorite = selected ? favorites.has(selected) : false;

  return (
    <div className="relative" ref={dropdownRef}>
      <p className="text-xs uppercase tracking-wide text-slate-400 mb-2">{label}</p>
      <button
        onClick={() => {
          const willOpen = !isOpen;
          setIsOpen(willOpen);
          if (willOpen) onOpen?.();
        }}
        role="combobox"
        aria-expanded={isOpen}
        aria-label={label}
        className={`w-full text-left px-3 py-2 rounded border transition flex items-center justify-between gap-2 ${
          selected
            ? "border-accent text-accent bg-accent/10"
            : "border-slate-800 hover:border-slate-700"
        }`}
      >
        <div className="flex items-center gap-2 flex-1 min-w-0">
          {isSelectedFavorite && (
            <Star className="w-3 h-3 fill-yellow-500 text-yellow-500 flex-shrink-0" />
          )}
          <span className="truncate">{displayValue}</span>
        </div>
        <ChevronDown
          className={`w-4 h-4 flex-shrink-0 transition-transform ${isOpen ? "rotate-180" : ""}`}
        />
      </button>

      <AnimatePresence>
        {isOpen && (
        <motion.div
          initial={{ opacity: 0, y: -4, scale: 0.98 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: -4, scale: 0.98 }}
          transition={{ duration: 0.15, ease: "easeOut" }}
          className="absolute z-50 w-full mt-1 bg-surface/95 glass border border-slate-800 rounded-md shadow-lg max-h-80 flex flex-col">
          {/* Search input */}
          <div className="p-2 border-b border-slate-800">
            <div className="relative">
              <Search className="w-4 h-4 text-slate-400 absolute left-2 top-1/2 -translate-y-1/2 z-10" />
              <input
                ref={searchInputRef}
                type="text"
                placeholder="Search..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Tab" && autocompleteSuggestion) {
                    e.preventDefault();
                    setSearchQuery(autocompleteSuggestion);
                  }
                }}
                className="w-full bg-slate-900/50 border border-slate-700 rounded px-8 py-1.5 text-sm text-slate-100 outline-none focus:border-accent transition relative z-10"
                onClick={(e) => e.stopPropagation()}
              />
              {/* Autocomplete suggestion overlay */}
              {autocompleteSuggestion && (
                <div className="absolute left-8 top-1/2 -translate-y-1/2 pointer-events-none text-slate-500 text-sm z-0">
                  <span className="invisible">{searchQuery}</span>
                  <span className="text-slate-600">{autocompleteSuggestion.slice(searchQuery.length)}</span>
                </div>
              )}
            </div>
          </div>

          {/* Items list */}
          <div className="overflow-y-auto flex-1">
            {sortedItems.length === 0 ? (
              <div className="px-3 py-2 text-sm text-slate-500">No items found</div>
            ) : (
              sortedItems.map((item, index) => {
                const isFavorite = favorites.has(item);
                const isSelected = selected === item;
                const isHighlighted = index === highlightedIndex && sortedItems.length > 0;
                const displayLabel = allOption && item === allOption.value ? allOption.label : item;

                return (
                  <button
                    key={item}
                    ref={(el) => {
                      itemRefs.current[index] = el;
                    }}
                    onClick={() => {
                      onSelect(item);
                      setIsOpen(false);
                      setSearchQuery("");
                      setHighlightedIndex(0);
                    }}
                    onMouseEnter={() => setHighlightedIndex(index)}
                    className={`w-full text-left px-3 py-2 flex items-center justify-between gap-2 transition ${
                      isHighlighted
                        ? "bg-accent/20 text-accent"
                        : isSelected
                        ? "bg-accent/10 text-accent"
                        : "text-slate-200 hover:bg-slate-800/50"
                    }`}
                  >
                    <div className="flex items-center gap-2 flex-1 min-w-0">
                      {isFavorite && (
                        <Star className="w-3 h-3 fill-yellow-500 text-yellow-500 flex-shrink-0" />
                      )}
                      <span className="truncate">
                        {highlightMatch(displayLabel, searchQuery)}
                      </span>
                    </div>
                    <button
                      onClick={(e) => toggleFavorite(item, e)}
                      className="flex-shrink-0 p-1 hover:bg-slate-700 rounded transition"
                      title={isFavorite ? "Remove from favorites" : "Add to favorites"}
                    >
                      <Star
                        className={`w-3 h-3 transition-colors ${
                          isFavorite
                            ? "fill-yellow-500 text-yellow-500"
                            : "text-slate-500 hover:text-yellow-500"
                        }`}
                      />
                    </button>
                  </button>
                );
              })
            )}
          </div>
        </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

