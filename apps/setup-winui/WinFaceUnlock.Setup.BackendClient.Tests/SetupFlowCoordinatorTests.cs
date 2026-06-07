using Xunit;
using WinFaceUnlock.Setup.BackendClient;

namespace WinFaceUnlock.Setup.BackendClient.Tests;

public sealed class SetupFlowCoordinatorTests
{
    [Fact]
    public void CreateReadinessCheckPlan_UsesOnlyPackageAndPreflightSteps()
    {
        var coordinator = CreateCoordinator();

        var plan = coordinator.CreateReadinessCheckPlan(new SetupReadinessCheckOptions(
            InstallDir: @"C:\Program Files\WinFaceUnlock",
            PayloadRootDir: @"C:\payload"));

        AssertPlan(
            plan,
            ("inspect", "inspect_package"),
            ("preflight", "source_preflight"));
    }

    [Fact]
    public void CreateInstallPlan_UsesUserFacingSetupSequence()
    {
        var coordinator = CreateCoordinator();

        var plan = coordinator.CreateInstallPlan(new SetupInstallPlanOptions
        {
            InstallDir = @"C:\Program Files\WinFaceUnlock",
            PayloadRootDir = @"C:\payload"
        });

        AssertPlan(
            plan,
            ("inspect", "inspect_package"),
            ("preflight", "source_preflight"),
            ("stage", "stage_payload"),
            ("preflight", "staged_preflight"),
            ("install", "install_components"));
    }

    [Fact]
    public void CreateRepairPlan_UsesComponentRepairSequence()
    {
        var coordinator = CreateCoordinator();

        var plan = coordinator.CreateRepairPlan(new SetupRepairPlanOptions
        {
            InstallDir = @"C:\Program Files\WinFaceUnlock",
            PayloadRootDir = @"C:\payload"
        });

        AssertPlan(
            plan,
            ("inspect", "inspect_package"),
            ("preflight", "source_preflight"),
            ("stage", "stage_payload"),
            ("preflight", "staged_preflight"),
            ("recovery", "repair_components"));
    }

    [Fact]
    public void ResolvePayloadSourcePath_LeavesAbsolutePathUnchanged()
    {
        var absolutePath = @"C:\payload\installer_cli.exe";

        var resolved = SetupFlowCoordinator.ResolvePayloadSourcePath(@"C:\ignored", absolutePath);

        Assert.Equal(absolutePath, resolved);
    }

    [Fact]
    public void ResolvePayloadSourcePath_JoinsRelativePathUnderPayloadRoot()
    {
        var resolved = SetupFlowCoordinator.ResolvePayloadSourcePath(
            @"C:\WinFaceUnlock\payload",
            @"models\face_detection_yunet_2023mar.onnx");

        Assert.Equal(
            @"C:\WinFaceUnlock\payload\models\face_detection_yunet_2023mar.onnx",
            resolved);
    }

    [Fact]
    public void ResolvePayloadSourcePath_RejectsRelativePathWithoutPayloadRoot()
    {
        Assert.Throws<InvalidOperationException>(() =>
            SetupFlowCoordinator.ResolvePayloadSourcePath("", @"models\face_detection_yunet_2023mar.onnx"));
    }

    private static SetupFlowCoordinator CreateCoordinator()
    {
        return new SetupFlowCoordinator(new SetupBackendClient(@"C:\payload\installer_cli.exe"));
    }

    private static void AssertPlan(
        IReadOnlyList<SetupFlowPlanStep> plan,
        params (string StepKey, string RunningMessageKey)[] expected)
    {
        Assert.Equal(expected.Length, plan.Count);
        for (var index = 0; index < expected.Length; index++)
        {
            Assert.Equal(expected[index].StepKey, plan[index].StepKey);
            Assert.Equal(expected[index].RunningMessageKey, plan[index].RunningMessageKey);
        }
    }
}
