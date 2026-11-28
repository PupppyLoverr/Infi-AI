// SPDX-License-Identifier: GPL-2.0-only
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
static PFN_vkCreateInstance vkCreateInstance;
static PFN_vkEnumeratePhysicalDevices vkEnumeratePhysicalDevices;
static PFN_vkGetPhysicalDeviceProperties vkGetPhysicalDeviceProperties;
static PFN_vkCreateDevice vkCreateDevice;
static PFN_vkGetDeviceQueue vkGetDeviceQueue;
static PFN_vkCreateBuffer vkCreateBuffer;
static PFN_vkGetBufferMemoryRequirements vkGetBufferMemoryRequirements;
static PFN_vkGetPhysicalDeviceMemoryProperties vkGetPhysicalDeviceMemoryProperties;
static PFN_vkAllocateMemory vkAllocateMemory;
static PFN_vkBindBufferMemory vkBindBufferMemory;
static PFN_vkMapMemory vkMapMemory;
static PFN_vkCreateShaderModule vkCreateShaderModule;
static PFN_vkCreateDescriptorSetLayout vkCreateDescriptorSetLayout;
static PFN_vkCreatePipelineLayout vkCreatePipelineLayout;
static PFN_vkCreateComputePipelines vkCreateComputePipelines;
static PFN_vkCreateDescriptorPool vkCreateDescriptorPool;
static PFN_vkAllocateDescriptorSets vkAllocateDescriptorSets;
static PFN_vkUpdateDescriptorSets vkUpdateDescriptorSets;
static PFN_vkCreateCommandPool vkCreateCommandPool;
static PFN_vkAllocateCommandBuffers vkAllocateCommandBuffers;
static PFN_vkBeginCommandBuffer vkBeginCommandBuffer;
static PFN_vkCmdBindPipeline vkCmdBindPipeline;
static PFN_vkCmdBindDescriptorSets vkCmdBindDescriptorSets;
static PFN_vkCmdDispatch vkCmdDispatch;
static PFN_vkCmdPipelineBarrier vkCmdPipelineBarrier;
static PFN_vkEndCommandBuffer vkEndCommandBuffer;
static PFN_vkCreateFence vkCreateFence;
static PFN_vkQueueSubmit vkQueueSubmit;
static PFN_vkWaitForFences vkWaitForFences;

/* The Vulkan entry points: from the loader (Linux) or straight from a driver (VT_ICD=path: the hosted macOS has no loader) */
static PFN_vkGetInstanceProcAddr gipa;
static void load_global(void) {
  const char *icd = getenv("VT_ICD");
  void *h = dlopen(icd ? icd : "libvulkan.so.1", RTLD_NOW | RTLD_LOCAL);
  if (!h) { printf("FAIL dlopen %s: %s\n", icd ? icd : "libvulkan.so.1", dlerror()); exit(1); }
  gipa = (PFN_vkGetInstanceProcAddr)dlsym(h, icd ? "vk_icdGetInstanceProcAddr" : "vkGetInstanceProcAddr");
  if (!gipa) { printf("FAIL no GetInstanceProcAddr\n"); exit(1); }
  vkCreateInstance = (PFN_vkCreateInstance)gipa(NULL, "vkCreateInstance");
}
static void load_instance(VkInstance inst) {
#define L(f) f = (PFN_##f)gipa(inst, #f); if (!f) { printf("FAIL no %s\n", #f); exit(1); }
  L(vkEnumeratePhysicalDevices)
  L(vkGetPhysicalDeviceProperties)
  L(vkCreateDevice)
  L(vkGetDeviceQueue)
  L(vkCreateBuffer)
  L(vkGetBufferMemoryRequirements)
  L(vkGetPhysicalDeviceMemoryProperties)
  L(vkAllocateMemory)
  L(vkBindBufferMemory)
  L(vkMapMemory)
  L(vkCreateShaderModule)
  L(vkCreateDescriptorSetLayout)
  L(vkCreatePipelineLayout)
  L(vkCreateComputePipelines)
  L(vkCreateDescriptorPool)
  L(vkAllocateDescriptorSets)
  L(vkUpdateDescriptorSets)
  L(vkCreateCommandPool)
  L(vkAllocateCommandBuffers)
  L(vkBeginCommandBuffer)
  L(vkCmdBindPipeline)
  L(vkCmdBindDescriptorSets)
  L(vkCmdDispatch)
  L(vkCmdPipelineBarrier)
  L(vkEndCommandBuffer)
  L(vkCreateFence)
  L(vkQueueSubmit)
  L(vkWaitForFences)
#undef L
}
#define CK(x) do{VkResult r=(x); if(r){printf("FAIL %s = %d\n",#x,r); exit(1);}}while(0)
int main(int argc,char**argv){
  const uint32_t N=1u<<22; /* 4M uints = 16 MB */
  if(argc<2){ printf("usage: %s compute.spv   (glslangValidator -V compute.comp -o compute.spv; VT_ICD=driver, VT_DEVICE=n)\n",argv[0]); return 2; }
  VkInstance inst; VkApplicationInfo ai={VK_STRUCTURE_TYPE_APPLICATION_INFO,0,"vt",1,0,0,VK_API_VERSION_1_1};
  VkInstanceCreateInfo ic={VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,0,0,&ai};
  load_global();
  CK(vkCreateInstance(&ic,0,&inst));
  load_instance(inst);
  /* the first device that is not a CPU, or VT_DEVICE=n */
  uint32_t n=8, pick=0; VkPhysicalDevice pds[8], pd; VkPhysicalDeviceProperties pp;
  VkResult er=vkEnumeratePhysicalDevices(inst,&n,pds);
  if((er!=VK_SUCCESS&&er!=VK_INCOMPLETE)||!n){ printf("FAIL vkEnumeratePhysicalDevices = %d, %u devices\n",er,n); return 1; }
  for(uint32_t i=0;i<n;i++){ vkGetPhysicalDeviceProperties(pds[i],&pp); printf("device %u: %s (type %d)\n",i,pp.deviceName,pp.deviceType); }
  for(uint32_t i=n;i-->0;){ vkGetPhysicalDeviceProperties(pds[i],&pp); if(pp.deviceType!=VK_PHYSICAL_DEVICE_TYPE_CPU) pick=i; }
  if(getenv("VT_DEVICE")) pick=(uint32_t)atoi(getenv("VT_DEVICE"))%n;
  pd=pds[pick]; vkGetPhysicalDeviceProperties(pd,&pp); printf("device: %s\n",pp.deviceName);
  float pr=1; VkDeviceQueueCreateInfo qc={VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,0,0,0,1,&pr};
  VkDeviceCreateInfo dc={VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,0,0,1,&qc};
  VkDevice dev; CK(vkCreateDevice(pd,&dc,0,&dev)); VkQueue q; vkGetDeviceQueue(dev,0,0,&q);
  VkBufferCreateInfo bc={VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,0,0,N*4,VK_BUFFER_USAGE_STORAGE_BUFFER_BIT};
  VkBuffer buf; CK(vkCreateBuffer(dev,&bc,0,&buf));
  VkMemoryRequirements mr; vkGetBufferMemoryRequirements(dev,buf,&mr);
  VkPhysicalDeviceMemoryProperties mp; vkGetPhysicalDeviceMemoryProperties(pd,&mp);
  uint32_t mt=~0u; for(uint32_t i=0;i<mp.memoryTypeCount;i++) if((mr.memoryTypeBits>>i&1)&&(mp.memoryTypes[i].propertyFlags&(VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT|VK_MEMORY_PROPERTY_HOST_COHERENT_BIT))==(VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT|VK_MEMORY_PROPERTY_HOST_COHERENT_BIT)){mt=i;break;}
  printf("memtype %u flags 0x%x\n",mt,mp.memoryTypes[mt].propertyFlags);
  VkMemoryAllocateInfo ma={VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,0,mr.size,mt}; VkDeviceMemory mem; CK(vkAllocateMemory(dev,&ma,0,&mem));
  CK(vkBindBufferMemory(dev,buf,mem,0)); uint32_t*p; CK(vkMapMemory(dev,mem,0,N*4,0,(void**)&p));
  for(uint32_t i=0;i<N;i++) p[i]=i;
  FILE*f=fopen(argv[1],"rb"); if(!f){ perror(argv[1]); return 1; } fseek(f,0,2); long sz=ftell(f); rewind(f); uint32_t*code=malloc(sz); if(fread(code,1,sz,f)!=(size_t)sz){printf("short read\n");return 1;} fclose(f);
  VkShaderModuleCreateInfo sc={VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,0,0,sz,code}; VkShaderModule sm; CK(vkCreateShaderModule(dev,&sc,0,&sm));
  VkDescriptorSetLayoutBinding lb={0,VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,1,VK_SHADER_STAGE_COMPUTE_BIT};
  VkDescriptorSetLayoutCreateInfo lc={VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,0,0,1,&lb}; VkDescriptorSetLayout dl; CK(vkCreateDescriptorSetLayout(dev,&lc,0,&dl));
  VkPipelineLayoutCreateInfo plc={VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,0,0,1,&dl}; VkPipelineLayout pl; CK(vkCreatePipelineLayout(dev,&plc,0,&pl));
  VkComputePipelineCreateInfo cpc={VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO,0,0,{VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,0,0,VK_SHADER_STAGE_COMPUTE_BIT,sm,"main"},pl};
  VkPipeline pipe; CK(vkCreateComputePipelines(dev,0,1,&cpc,0,&pipe));
  VkDescriptorPoolSize ps={VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,1}; VkDescriptorPoolCreateInfo dpc={VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,0,0,1,1,&ps}; VkDescriptorPool dp; CK(vkCreateDescriptorPool(dev,&dpc,0,&dp));
  VkDescriptorSetAllocateInfo dsa={VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,0,dp,1,&dl}; VkDescriptorSet ds; CK(vkAllocateDescriptorSets(dev,&dsa,&ds));
  VkDescriptorBufferInfo dbi={buf,0,VK_WHOLE_SIZE}; VkWriteDescriptorSet w={VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,0,ds,0,0,1,VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,0,&dbi}; vkUpdateDescriptorSets(dev,1,&w,0,0);
  VkCommandPoolCreateInfo cpi={VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,0,0,0}; VkCommandPool cp; CK(vkCreateCommandPool(dev,&cpi,0,&cp));
  VkCommandBufferAllocateInfo cba={VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,0,cp,0,1}; VkCommandBuffer cb; CK(vkAllocateCommandBuffers(dev,&cba,&cb));
  VkCommandBufferBeginInfo bi={VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO}; CK(vkBeginCommandBuffer(cb,&bi));
  vkCmdBindPipeline(cb,VK_PIPELINE_BIND_POINT_COMPUTE,pipe); vkCmdBindDescriptorSets(cb,VK_PIPELINE_BIND_POINT_COMPUTE,pl,0,1,&ds,0,0);
  vkCmdDispatch(cb,1024,N/65536,1);	/* 1024 x 64 groups of 64 (compute.comp): N/64 in one row is 65536, and 65535 is all a device has to take (lavapipe's limit) */
  VkMemoryBarrier mb={VK_STRUCTURE_TYPE_MEMORY_BARRIER,0,VK_ACCESS_SHADER_WRITE_BIT,VK_ACCESS_HOST_READ_BIT};
  vkCmdPipelineBarrier(cb,VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,VK_PIPELINE_STAGE_HOST_BIT,0,1,&mb,0,0,0,0);
  CK(vkEndCommandBuffer(cb));
  VkFenceCreateInfo fc={VK_STRUCTURE_TYPE_FENCE_CREATE_INFO}; VkFence fe; CK(vkCreateFence(dev,&fc,0,&fe));
  struct timespec t0,t1; clock_gettime(CLOCK_MONOTONIC,&t0);
  VkSubmitInfo si={VK_STRUCTURE_TYPE_SUBMIT_INFO,0,0,0,0,1,&cb}; CK(vkQueueSubmit(q,1,&si,fe)); CK(vkWaitForFences(dev,1,&fe,1,~0ull));
  clock_gettime(CLOCK_MONOTONIC,&t1);
  uint32_t bad=0; for(uint32_t i=0;i<N;i++) if(p[i]!=i*2u+1u){ if(bad<3) printf("mismatch %u: %u\n",i,p[i]); bad++; }
  printf("compute %u elems: %s (%u bad), %.2f ms\n",N,bad?"FAIL":"OK",bad,(t1.tv_sec-t0.tv_sec)*1e3+(t1.tv_nsec-t0.tv_nsec)/1e6);
  if (bad) {	/* tell a wait that returned early from results that never arrive */
    struct timespec d = { 1, 0 }; nanosleep(&d, NULL);
    uint32_t late = 0; for(uint32_t i=0;i<N;i++) if(p[i]!=i*2u+1u) late++;
    printf("after 1 s more: %u bad\n", late);
  }
  return bad?1:0;
}
