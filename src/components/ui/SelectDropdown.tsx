import React, { useState, useRef, useEffect } from "react";
import { ChevronDown, Check, Search } from "lucide-react";
import { cn } from "@/lib/utils";

export interface SelectOption {
  value: string;
  label: string;
  sublabel?: string;
  badge?: string;
  badgeColor?: string;
  icon?: React.ReactNode;
}

interface SelectDropdownProps {
  options: SelectOption[];
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  searchable?: boolean;
  className?: string;
  triggerClassName?: string;
  menuClassName?: string;
  disabled?: boolean;
}

export const SelectDropdown: React.FC<SelectDropdownProps> = ({
  options,
  value,
  onChange,
  placeholder = "Select an option...",
  searchable = false,
  className,
  triggerClassName,
  menuClassName,
  disabled = false,
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const containerRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  const selectedOption = options.find((opt) => opt.value === value);

  // Close on click outside
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setIsOpen(false);
      }
    };

    if (isOpen) {
      document.addEventListener("mousedown", handleClickOutside);
    }
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [isOpen]);

  // Focus search input on open
  useEffect(() => {
    if (isOpen && searchable && searchInputRef.current) {
      searchInputRef.current.focus();
    }
    if (!isOpen) {
      setSearchQuery("");
    }
  }, [isOpen, searchable]);

  // Keyboard navigation: ESC closes
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      setIsOpen(false);
    }
  };

  const filteredOptions = searchable && searchQuery
    ? options.filter(
        (opt) =>
          opt.label.toLowerCase().includes(searchQuery.toLowerCase()) ||
          (opt.sublabel && opt.sublabel.toLowerCase().includes(searchQuery.toLowerCase()))
      )
    : options;

  return (
    <div
      ref={containerRef}
      onKeyDown={handleKeyDown}
      data-no-drag="true"
      className={cn("relative inline-block text-left select-none", className)}
    >
      {/* Trigger Button */}
      <button
        type="button"
        disabled={disabled}
        onClick={() => setIsOpen(!isOpen)}
        onMouseDown={(e) => e.stopPropagation()}
        className={cn(
          "flex items-center justify-between gap-2 px-3 py-1.5 rounded-lg text-xs font-medium bg-zinc-900/90 border border-zinc-800 hover:border-zinc-700/80 text-zinc-200 transition-all cursor-pointer shadow-xs focus:outline-none focus:ring-1 focus:ring-blue-500/50",
          isOpen && "border-blue-500/60 ring-1 ring-blue-500/50",
          disabled && "opacity-50 cursor-not-allowed",
          triggerClassName
        )}
      >
        <div className="flex items-center gap-2 truncate text-left">
          {selectedOption?.icon && (
            <span className="shrink-0 text-zinc-400">{selectedOption.icon}</span>
          )}
          <span className="truncate">{selectedOption ? selectedOption.label : placeholder}</span>
          {selectedOption?.badge && (
            <span
              className={cn(
                "text-[10px] uppercase font-bold px-1.5 py-0.2 rounded border font-mono shrink-0",
                selectedOption.badgeColor || "bg-zinc-800 text-zinc-400 border-zinc-750"
              )}
            >
              {selectedOption.badge}
            </span>
          )}
        </div>

        <ChevronDown
          className={cn(
            "w-3.5 h-3.5 text-zinc-400 shrink-0 transition-transform duration-200",
            isOpen && "rotate-180 text-blue-400"
          )}
        />
      </button>

      {/* Popover Dropdown Menu */}
      {isOpen && (
        <div
          data-no-drag="true"
          onMouseDown={(e) => e.stopPropagation()}
          className={cn(
            "absolute z-50 mt-1 min-w-[200px] max-w-[340px] rounded-xl bg-zinc-900/95 backdrop-blur-2xl border border-white/[0.08] shadow-2xl p-1.5 space-y-1 animate-in fade-in zoom-in-95 duration-100",
            menuClassName
          )}
        >
          {/* Optional Search */}
          {searchable && (
            <div className="px-2 py-1.5 border-b border-zinc-800/80 flex items-center gap-2 mb-1">
              <Search className="w-3.5 h-3.5 text-zinc-400 shrink-0" />
              <input
                ref={searchInputRef}
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Search..."
                className="w-full bg-transparent text-xs text-zinc-100 placeholder-zinc-500 focus:outline-none"
              />
            </div>
          )}

          {/* Options List */}
          <div className="max-h-56 overflow-y-auto space-y-0.5 custom-scrollbar">
            {filteredOptions.length === 0 ? (
              <div className="px-3 py-3 text-center text-xs text-zinc-500 italic">
                No matching options
              </div>
            ) : (
              filteredOptions.map((opt) => {
                const isSelected = opt.value === value;
                return (
                  <button
                    key={opt.value}
                    type="button"
                    onClick={() => {
                      onChange(opt.value);
                      setIsOpen(false);
                    }}
                    className={cn(
                      "w-full flex items-center justify-between px-2.5 py-1.5 rounded-lg text-xs transition cursor-pointer text-left group",
                      isSelected
                        ? "bg-blue-600/20 text-blue-300 font-medium"
                        : "text-zinc-300 hover:bg-zinc-800/80 hover:text-white"
                    )}
                  >
                    <div className="flex items-center gap-2 truncate">
                      {opt.icon && (
                        <span className="shrink-0 text-zinc-400 group-hover:text-zinc-200">
                          {opt.icon}
                        </span>
                      )}
                      <div className="truncate">
                        <div className="truncate leading-snug">{opt.label}</div>
                        {opt.sublabel && (
                          <div className="text-[10px] text-zinc-500 truncate leading-none mt-0.5">
                            {opt.sublabel}
                          </div>
                        )}
                      </div>
                    </div>

                    <div className="flex items-center gap-1.5 shrink-0 ml-2">
                      {opt.badge && (
                        <span
                          className={cn(
                            "text-[9px] uppercase font-bold px-1.2 py-0.2 rounded border font-mono",
                            opt.badgeColor || "bg-zinc-800 text-zinc-400 border-zinc-750"
                          )}
                        >
                          {opt.badge}
                        </span>
                      )}
                      {isSelected && <Check className="w-3.5 h-3.5 text-blue-400 shrink-0" />}
                    </div>
                  </button>
                );
              })
            )}
          </div>
        </div>
      )}
    </div>
  );
};
