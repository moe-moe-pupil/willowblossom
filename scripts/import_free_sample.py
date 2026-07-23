"""Convert the user-supplied FreeSample FBX pack to Bevy-ready glTF assets.

Run with Blender 4.2 or newer:
    blender --background --python scripts/import_free_sample.py -- SOURCE_DIR OUTPUT_DIR
"""

import sys
from pathlib import Path

import bpy


MODEL_TEXTURES = {
    "BuildingBlock_2": "Props",
    "Lander": "Vehicles",
    "Prop_14": "Props",
    "Prop_15": "Props",
    "SatelliteDish_1": "Props",
    "SolarPanel_4": "Props",
}


def arguments():
    separator = sys.argv.index("--")
    return Path(sys.argv[separator + 1]), Path(sys.argv[separator + 2])


def configure_material(material, diffuse_path, emissive_path):
    material.use_nodes = True
    nodes = material.node_tree.nodes
    nodes.clear()
    output = nodes.new("ShaderNodeOutputMaterial")
    shader = nodes.new("ShaderNodeBsdfPrincipled")
    diffuse = nodes.new("ShaderNodeTexImage")
    diffuse.image = bpy.data.images.load(str(diffuse_path), check_existing=True)
    diffuse.interpolation = "Linear"
    material.node_tree.links.new(diffuse.outputs["Color"], shader.inputs["Base Color"])

    if emissive_path.exists():
        emissive = nodes.new("ShaderNodeTexImage")
        emissive.image = bpy.data.images.load(str(emissive_path), check_existing=True)
        emissive.interpolation = "Linear"
        material.node_tree.links.new(emissive.outputs["Color"], shader.inputs["Emission Color"])
        shader.inputs["Emission Strength"].default_value = 2.5

    shader.inputs["Metallic"].default_value = 0.35
    shader.inputs["Roughness"].default_value = 0.48
    material.node_tree.links.new(shader.outputs["BSDF"], output.inputs["Surface"])


def convert_model(source_dir, output_dir, model_name, texture_set):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.fbx(filepath=str(source_dir / f"{model_name}.fbx"))

    texture_root = source_dir.parent / "Textures" / "Bakes"
    diffuse_path = texture_root / f"T_{texture_set}_diffuse.png"
    emissive_path = texture_root / f"T_{texture_set}_emissive.png"
    for material in bpy.data.materials:
        configure_material(material, diffuse_path, emissive_path)

    for obj in bpy.context.scene.objects:
        if obj.type == "MESH":
            obj.name = f"{model_name}_{obj.name}"

    bpy.ops.export_scene.gltf(
        filepath=str(output_dir / f"{model_name}.gltf"),
        export_format="GLTF_SEPARATE",
        export_texture_dir="textures",
        export_image_format="AUTO",
        export_keep_originals=False,
        export_yup=True,
        export_apply=True,
        export_animations=False,
    )


def main():
    source_dir, output_dir = arguments()
    output_dir.mkdir(parents=True, exist_ok=True)
    (output_dir / "textures").mkdir(exist_ok=True)
    for model_name, texture_set in MODEL_TEXTURES.items():
        convert_model(source_dir, output_dir, model_name, texture_set)

    note = output_dir / "SOURCE.txt"
    note.write_text(
        "Converted from the user-supplied FreeSample.zip.\n"
        "The archive contained no author, URL, or license file; verify redistribution rights "
        "before publishing these assets.\n",
        encoding="utf-8",
    )


main()
