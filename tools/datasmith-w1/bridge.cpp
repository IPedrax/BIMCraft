#define FVector AppleFVector
#import <Foundation/Foundation.h>
#undef FVector
#include "DatasmithMesh.h"
#include "DatasmithMeshExporter.h"
#include "DatasmithSceneExporter.h"
#include "DatasmithSceneFactory.h"
#include "DatasmithMaterialElements.h"

// The caller decodes the fixture’s BIMCraft Rust GLB and pairs each primitive with
// its persisted model identity. Positions are local left-handed Z-up centimetres.
struct Part
{
    const TCHAR* actor;
    const TCHAR* mesh;
    const TCHAR* material;
    const TCHAR* label;
    const float* positions;
    const float* normals;
    const float* uvs;
    const int32* indices;
    int32 vertexCount;
    int32 indexCount;
    double pivot[3];
    float rgba[4];
};

extern "C" __attribute__((visibility("default")))
int WriteW1Scene(const TCHAR* folder, const TCHAR* version, const Part* parts, int32 count)
{
    FDatasmithSceneExporter writer;
    writer.SetName(TEXT("CON03_W1"));
    writer.SetOutputPath(folder);
    writer.PreExport();
    auto scene = FDatasmithSceneFactory::CreateScene(TEXT("CON03_W1"));
    scene->SetHost(TEXT("BIMCraft Rust"));
    scene->SetProductName(TEXT("BIMCraft Rust"));
    scene->SetProductVersion(version);
    scene->SetVendor(TEXT("Local prototype"));

    for (int32 i = 0; i < count; ++i)
    {
        const Part& part = parts[i];
        auto material = FDatasmithSceneFactory::CreateUEPbrMaterial(part.material);
        material->SetLabel(part.label);
        auto color = material->AddMaterialExpression<IDatasmithMaterialExpressionColor>();
        color->GetColor() = FLinearColor(part.rgba[0], part.rgba[1], part.rgba[2]);
        color->ConnectExpression(material->GetBaseColor());
        material->SetTwoSided(true);
        if (part.rgba[3] < 1.0f)
        {
            material->SetBlendMode(2); // BLEND_Translucent
            auto opacity = material->AddMaterialExpression<IDatasmithMaterialExpressionScalar>();
            opacity->GetScalar() = part.rgba[3];
            opacity->ConnectExpression(material->GetOpacity());
        }
        scene->AddMaterial(material);

        FDatasmithMesh mesh;
        mesh.SetName(part.mesh);
        mesh.SetVerticesCount(part.vertexCount);
        mesh.SetFacesCount(part.indexCount / 3);
        mesh.SetUVChannelsCount(1);
        mesh.SetUVCount(0, part.vertexCount);
        for (int32 vertex = 0; vertex < part.vertexCount; ++vertex)
        {
            const float* v = part.positions + 3 * vertex;
            mesh.SetVertex(vertex, v[0], v[1], v[2]);
            mesh.SetUV(0, vertex, part.uvs[2 * vertex], part.uvs[2 * vertex + 1]);
        }
        for (int32 corner = 0; corner < part.indexCount; corner += 3)
        {
            const int32* tri = part.indices + corner;
            mesh.SetFace(corner / 3, tri[0], tri[1], tri[2], 0);
            mesh.SetFaceUV(corner / 3, 0, tri[0], tri[1], tri[2]);
            for (int32 k = 0; k < 3; ++k)
            {
                const float* n = part.normals + 3 * tri[k];
                mesh.SetNormal(corner + k, n[0], n[1], n[2]);
            }
        }
        FDatasmithMeshExporter meshWriter;
        auto asset = meshWriter.ExportToUObject(writer.GetAssetsOutputPath(), part.mesh,
                                                mesh, nullptr, EDSExportLightmapUV::Never);
        if (!asset)
        {
            return 1;
        }
        asset->SetLabel(part.label);
        asset->SetMaterial(part.material, 0);
        scene->AddMesh(asset.ToSharedRef());
        auto actor = FDatasmithSceneFactory::CreateMeshActor(part.actor);
        actor->SetLabel(part.label);
        actor->SetStaticMeshPathName(part.mesh);
        actor->SetTranslation(part.pivot[0], part.pivot[1], part.pivot[2]);
        scene->AddActor(actor);
    }
    writer.Export(scene);
    return 0;
}
