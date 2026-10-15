import { useQuery } from "@tanstack/react-query";
import { commands } from "./bindings";

export function useRecipes() {
  return useQuery({ queryKey: ["recipes"], queryFn: () => commands.recipesList() });
}

export function useRecipeState(bottleId: string) {
  return useQuery({
    queryKey: ["bottles", bottleId, "recipe-state"],
    queryFn: () => commands.recipeState(bottleId),
  });
}

/** Bottle being set up by each recipe in this session (recipe id -> bottle id). */
export const recipeBottles = new Map<string, string>();
