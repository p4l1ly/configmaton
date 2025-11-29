import asyncio
from dataclasses import dataclass
from configmaton import Configmaton


@dataclass
class Ingredient:
    name: str
    amount: int


async def main():
    import os

    test_dir = os.path.dirname(__file__)
    kitchen_path = os.path.join(test_dir, "kitchen.json")
    with open(kitchen_path, "rb") as f:
        cfg = Configmaton(f.read(), handle_command)

    def get_present_ingredients():
        return [
            Ingredient(name="tomato", amount=10),
            Ingredient(name="onion", amount=5),
            Ingredient(name="garlic", amount=3),
            Ingredient(name="salt", amount=1),
            Ingredient(name="pepper", amount=1),
            Ingredient(name="olive oil", amount=1),
            Ingredient(name="vinegar", amount=1),
            Ingredient(name="flour", amount=1),
            Ingredient(name="yeast", amount=1),
        ]

    def get_cooking_history():
        return ["tomato soup", "dumpling", "pizza"]

    def choose_recipe(ingredients, history):
        print(f"Choosing recipe for {ingredients} and history {history}")
        recipe = input()
        print(f"Choose amount of {recipe}")
        amount = float(input())
        return recipe, amount

    def get_missing_ingredients(ingredients, recipe, recipe_amount):
        return []

    def go_get_ingredients(ingredients):
        raise NotImplementedError()

    def update_ingredients(ingredients, new_ingredients):
        raise NotImplementedError()

    def make_dough():
        cfg["process"] = "dough"
        cfg["step"] = "starter"
        print(f"put {cfg['water_amount']} water into a bowl")
        print(f"add {cfg['flour_amount']} flour into a bowl")
        print(f"add yeast")

    ingredients = await get_present_ingredients()
    history = await get_cooking_history()
    recipe, recipe_amount = await choose_recipe(ingredients, history)
    cfg["recipe"] = recipe

    if missing_ingredients := get_missing_ingredients(ingredients, recipe, recipe_amount):
        new_ingredients = await go_get_ingredients(missing_ingredients)
        ingredients = update_ingredients(ingredients, new_ingredients)

    if recipe == "tomato soup":
        print("Making tomato soup")
        print(f"cut {cfg['onion_amount']} onions")
        print(f"put it on heated oil")
        print(f"cut {cfg['tomato_amount']} tomatoes")
        print(f"cut {cfg['garlic_amount']} garlic")
        print(f"put tomatoes and garlic on heated oil")
        print("etc.")
        return

    if recipe == "dumpling":
        make_dough()
        print("steam the dough")

    if recipe == "pizza":
        make_dough()
        print(f"put on {cfg['sugo_amount']} sugo")
        print(f"put on {cfg['mozzarella_amount']} mozzarella")
        print(f"put on {cfg['basil_amount']} basil")
        print(f"bake at {cfg['bake_temperature']}°C for {cfg['bake_time']} minutes")
        return


asyncio.run(main())
